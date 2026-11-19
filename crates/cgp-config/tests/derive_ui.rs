//! Compile-fail tests for `#[derive(ConfigSection)]` misuse (trybuild). Regenerate the
//! expected output with `TRYBUILD=overwrite cargo test -p cgp-config --test derive_ui`
//! after reviewing a change in wording.

#![cfg(all(feature = "derive", feature = "std"))]

#[test]
fn derive_ui() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui/pass_*.rs");
    cases.compile_fail("tests/ui/fail_*.rs");
}
