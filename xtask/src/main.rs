//! Repository tasks. `cargo xtask ci` runs the checks CI runs, in the same order.
//!
//! ```text
//! cargo xtask ci          # fmt, clippy, test, feature matrix, no_std, docs, deny
//! cargo xtask fmt         # rustfmt check (nightly, for the unstable options in .rustfmt.toml)
//! cargo xtask clippy      # clippy -D warnings, all features and no default features
//! cargo xtask test        # tests, all features and each feature tier of the facade
//! cargo xtask hack        # cargo-hack: each feature (facade), powerset (core, value)
//! cargo xtask nostd       # build core and value for thumbv7em-none-eabihf
//! cargo xtask docs        # rustdoc with broken intra-doc links denied
//! cargo xtask deny        # cargo-deny licenses, bans, advisories, sources
//! cargo xtask miri        # Miri on the value crate (nightly)
//! cargo xtask coverage    # llvm-cov, fail under 95% lines (value; core + value)
//! ```
//!
//! The tools beyond rustup components (`cargo-hack`, `cargo-deny`, `cargo-llvm-cov`) must
//! be installed; a missing tool fails its step with an install hint.

use std::env;
use std::process::{Command, ExitCode};

/// The embedded target used to prove `no_std` builds.
const NO_STD_TARGET: &str = "thumbv7em-none-eabihf";
/// Minimum line coverage of `cgp-config-value`.
const VALUE_COVERAGE: &str = "95";

type Step = fn() -> Result<(), String>;

const STEPS: &[(&str, Step, &str)] = &[
    ("fmt", fmt, "rustfmt check"),
    ("clippy", clippy, "clippy, warnings denied"),
    ("test", test, "tests in every feature tier"),
    ("hack", hack, "feature matrix with cargo-hack"),
    ("nostd", nostd, "no_std builds for an embedded target"),
    ("docs", docs, "rustdoc, broken links denied"),
    ("deny", deny, "cargo-deny"),
    ("miri", miri, "Miri on the value crate"),
    ("coverage", coverage, "line coverage gate"),
];

/// Steps of `ci`. Miri and coverage run in their own CI jobs.
const CI: &[&str] = &["fmt", "clippy", "test", "hack", "nostd", "docs", "deny"];

fn main() -> ExitCode {
    let task = env::args().nth(1).unwrap_or_else(|| "help".to_owned());
    let result = match task.as_str() {
        "ci" => CI.iter().try_for_each(|name| run_step(name)),
        "help" | "-h" | "--help" => {
            help();
            Ok(())
        }
        name => run_step(name),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn help() {
    println!(
        "usage: cargo xtask <task>\n\n  ci         {}",
        CI.join(", ")
    );
    for (name, _, about) in STEPS {
        println!("  {name:<10} {about}");
    }
}

fn run_step(name: &str) -> Result<(), String> {
    let (_, step, about) = STEPS
        .iter()
        .find(|(step, _, _)| *step == name)
        .ok_or_else(|| format!("unknown task `{name}`; run `cargo xtask help`"))?;
    eprintln!("\n==> {name}: {about}");
    step()
}

/// Runs `cargo` with `args`, optionally on a named toolchain (`+nightly`).
fn cargo(toolchain: Option<&str>, args: &[&str]) -> Result<(), String> {
    cargo_env(toolchain, &[], args)
}

/// [`cargo`] with extra environment variables for the child.
fn cargo_env(
    toolchain: Option<&str>,
    vars: &[(&str, String)],
    args: &[&str],
) -> Result<(), String> {
    let mut command = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()));
    if let Some(toolchain) = toolchain {
        // `+toolchain` is handled by the rustup proxy, not by the cargo binary itself.
        command = Command::new("cargo");
        command.arg(format!("+{toolchain}"));
        command
            .env_remove("RUSTC")
            .env_remove("RUSTDOC")
            .env_remove("CARGO");
    }
    command.args(args);
    command.envs(vars.iter().map(|(key, value)| (*key, value.as_str())));
    eprintln!(
        "    $ cargo {}{}",
        toolchain.map(|t| format!("+{t} ")).unwrap_or_default(),
        args.join(" ")
    );
    let status = command
        .status()
        .map_err(|error| format!("cannot run cargo: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`cargo {}` failed ({status})", args.join(" ")))
    }
}

fn require(subcommand: &str, install: &str) -> Result<(), String> {
    let found = Command::new("cargo")
        .args([subcommand, "--version"])
        .output()
        .is_ok_and(|output| output.status.success());
    if found {
        Ok(())
    } else {
        Err(format!(
            "`cargo {subcommand}` is not installed; install with `{install}`"
        ))
    }
}

fn fmt() -> Result<(), String> {
    cargo(Some("nightly"), &["fmt", "--all", "--check"])
}

fn clippy() -> Result<(), String> {
    let deny = ["--", "-D", "warnings"];
    for features in [&["--all-features"][..], &["--no-default-features"][..]] {
        let mut args = vec!["clippy", "--workspace", "--all-targets"];
        args.extend_from_slice(features);
        args.extend_from_slice(&deny);
        cargo(None, &args)?;
    }
    Ok(())
}

fn test() -> Result<(), String> {
    cargo(None, &["test", "--workspace", "--all-features"])?;
    cargo(
        None,
        &[
            "test",
            "-p",
            "cgp-config-core",
            "--no-default-features",
            "--lib",
        ],
    )?;
    cargo(
        None,
        &[
            "test",
            "-p",
            "cgp-config-core",
            "--no-default-features",
            "--features",
            "alloc",
        ],
    )?;
    cargo(
        None,
        &[
            "test",
            "-p",
            "cgp-config",
            "--no-default-features",
            "--features",
            "alloc",
        ],
    )
}

fn hack() -> Result<(), String> {
    require("hack", "cargo install cargo-hack --locked")?;
    cargo(
        None,
        &[
            "hack",
            "check",
            "-p",
            "cgp-config",
            "--each-feature",
            "--no-dev-deps",
        ],
    )?;
    cargo(
        None,
        &[
            "hack",
            "check",
            "-p",
            "cgp-config-core",
            "-p",
            "cgp-config-value",
            "--feature-powerset",
            "--no-dev-deps",
        ],
    )
}

fn nostd() -> Result<(), String> {
    let target = ["--target", NO_STD_TARGET];
    for args in [
        &["build", "-p", "cgp-config-core", "--no-default-features"][..],
        &[
            "build",
            "-p",
            "cgp-config-core",
            "--no-default-features",
            "--features",
            "alloc",
        ][..],
        &["build", "-p", "cgp-config-value", "--no-default-features"][..],
        &["build", "-p", "cgp-config", "--no-default-features"][..],
        &[
            "build",
            "-p",
            "cgp-config",
            "--no-default-features",
            "--features",
            "alloc",
        ][..],
    ] {
        let mut all = args.to_vec();
        all.extend_from_slice(&target);
        cargo(None, &all)?;
    }
    Ok(())
}

fn docs() -> Result<(), String> {
    cargo_env(
        None,
        &[(
            "RUSTDOCFLAGS",
            "-D warnings -D rustdoc::broken_intra_doc_links".to_owned(),
        )],
        &[
            "doc",
            "--workspace",
            "--no-deps",
            "--all-features",
            "--exclude",
            "xtask",
        ],
    )
}

fn deny() -> Result<(), String> {
    require("deny", "cargo install cargo-deny --locked")?;
    cargo(None, &["deny", "--workspace", "check"])
}

fn miri() -> Result<(), String> {
    // proptest reads its failure-persistence files, which Miri's isolation forbids.
    let flags = format!(
        "{} -Zmiri-disable-isolation",
        env::var("MIRIFLAGS").unwrap_or_default()
    );
    cargo_env(
        Some("nightly"),
        &[("MIRIFLAGS", flags)],
        &[
            "miri",
            "test",
            "-p",
            "cgp-config-value",
            "-p",
            "cgp-config-core",
            "--all-features",
        ],
    )
}

fn coverage() -> Result<(), String> {
    require("llvm-cov", "cargo install cargo-llvm-cov --locked")?;
    // Stale profiles from earlier builds skew the numbers; start clean.
    cargo(None, &["llvm-cov", "clean", "--workspace"])?;
    // The gate is on the value crate (the plan's target). Core is measured in the same run
    // so its numbers show up in the report; its providers are exercised by value's tests.
    cargo(
        None,
        &[
            "llvm-cov",
            "-p",
            "cgp-config-value",
            "--all-features",
            "--summary-only",
            "--fail-under-lines",
            VALUE_COVERAGE,
        ],
    )?;
    cargo(
        None,
        &[
            "llvm-cov",
            "-p",
            "cgp-config-core",
            "-p",
            "cgp-config-value",
            "--all-features",
            "--summary-only",
            "--fail-under-lines",
            VALUE_COVERAGE,
        ],
    )
}
