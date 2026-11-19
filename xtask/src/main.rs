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
//! cargo xtask miri        # Miri on core and value (nightly), isolation kept on
//! cargo xtask coverage    # llvm-cov: gate 95% lines on value; core + value printed only
//! cargo xtask semver      # cargo-semver-checks against $SEMVER_BASELINE (default origin/main)
//! cargo xtask fork-main   # build and test against cgp-fork's main branch (early warning)
//! ```
//!
//! The tools beyond rustup components (`cargo-hack`, `cargo-deny`, `cargo-llvm-cov`,
//! `cargo-semver-checks`) must be installed; a missing tool fails its step with an install
//! hint.

use std::process::{Command, ExitCode};
use std::{env, fs};

/// The embedded target used to prove `no_std` builds.
const NO_STD_TARGET: &str = "thumbv7em-none-eabihf";
/// Minimum line coverage of `cgp-config-value`.
const VALUE_COVERAGE: &str = "95";
/// The `cgp-fork` repository; `fork-main` builds against its default branch.
const CGP_FORK_GIT: &str = "https://github.com/Just-Replicant/cgp-fork";
/// The published crates checked by `semver`.
const PUBLISHED: &[&str] = &[
    "cgp-config",
    "cgp-config-core",
    "cgp-config-value",
    "cgp-config-formats",
    "cgp-config-providers",
    "cgp-config-derive",
    "cgp-config-derive-impl",
];

type Step = fn() -> Result<(), String>;

const STEPS: &[(&str, Step, &str)] = &[
    ("fmt", fmt, "rustfmt check"),
    ("clippy", clippy, "clippy, warnings denied"),
    ("test", test, "tests in every feature tier"),
    ("hack", hack, "feature matrix with cargo-hack"),
    ("nostd", nostd, "no_std builds for an embedded target"),
    ("docs", docs, "rustdoc, broken links denied"),
    ("deny", deny, "cargo-deny"),
    ("miri", miri, "Miri on core and value"),
    ("coverage", coverage, "line coverage gate"),
    (
        "semver",
        semver,
        "cargo-semver-checks against a baseline rev",
    ),
    (
        "fork-main",
        fork_main,
        "build and test against cgp-fork main",
    ),
];

/// Steps of `ci`. Miri, coverage, semver, and fork-main run in their own CI jobs.
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
            "-p",
            "cgp-config-formats",
            "--feature-powerset",
            "--no-dev-deps",
        ],
    )?;
    // The std crate has more features than a powerset can afford; each one alone is the
    // useful check (default features must not need dotenv or clap).
    cargo(
        None,
        &[
            "hack",
            "check",
            "-p",
            "cgp-config-providers",
            "--each-feature",
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
        &["build", "-p", "cgp-config-formats", "--no-default-features"][..],
        &[
            "build",
            "-p",
            "cgp-config-formats",
            "--no-default-features",
            "--features",
            "toml,json",
        ][..],
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
    // Isolation stays on: no test may touch the host file system, clock, or environment.
    // proptest's failure persistence is the one thing that would (it reads and writes
    // `*.proptest-regressions` files), so it is switched off instead of lifting isolation.
    // Isolation also hides the host environment, so that one variable is forwarded.
    let flags = format!(
        "{} -Zmiri-env-forward=PROPTEST_DISABLE_FAILURE_PERSISTENCE",
        env::var("MIRIFLAGS").unwrap_or_default()
    );
    cargo_env(
        Some("nightly"),
        &[
            ("PROPTEST_DISABLE_FAILURE_PERSISTENCE", "1".to_owned()),
            ("MIRIFLAGS", flags.trim_start().to_owned()),
        ],
        &[
            "miri",
            "test",
            "-p",
            "cgp-config-value",
            "-p",
            "cgp-config-core",
            "-p",
            "cgp-config-formats",
            "--all-features",
        ],
    )
}

fn coverage() -> Result<(), String> {
    require("llvm-cov", "cargo install cargo-llvm-cov --locked")?;
    // Stale profiles from earlier builds skew the numbers; start clean.
    cargo(None, &["llvm-cov", "clean", "--workspace"])?;
    // The gate is on the value crate only (the plan's target).
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
    // Core plus value is printed for information, not gated: core's providers are mostly
    // exercised through value's tests, and its no-alloc paths are not measurable here.
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
        ],
    )
}

fn semver() -> Result<(), String> {
    require(
        "semver-checks",
        "cargo install cargo-semver-checks --locked",
    )?;
    // Nothing is on crates.io before 0.1, so the baseline is a git revision: the PR's
    // base branch in CI, `origin/main` locally.
    let baseline = env::var("SEMVER_BASELINE").unwrap_or_else(|_| "origin/main".to_owned());
    let mut args = vec![
        "semver-checks",
        "check-release",
        "--baseline-rev",
        &baseline,
    ];
    for package in PUBLISHED {
        args.extend_from_slice(&["--package", package]);
    }
    cargo(None, &args)
}

fn fork_main() -> Result<(), String> {
    // Patch `cgp-fork` (and so its macro crates) to the fork's default branch. Cargo
    // rewrites Cargo.lock for the patched source; the lock file is restored afterwards so
    // a local run leaves the tree unchanged.
    let patch = format!("patch.crates-io.cgp-fork.git=\"{CGP_FORK_GIT}\"");
    let lock_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.lock");
    let lock = fs::read(lock_path).map_err(|error| format!("cannot read Cargo.lock: {error}"))?;
    let result = ["build", "test"].iter().try_for_each(|command| {
        cargo(
            None,
            &["--config", &patch, command, "--workspace", "--all-features"],
        )
    });
    fs::write(lock_path, lock).map_err(|error| format!("cannot restore Cargo.lock: {error}"))?;
    result
}
