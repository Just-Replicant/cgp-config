//! The `${...}` lexer never panics on any input.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = cgp_config_value::interpolate::lex(text);
});
