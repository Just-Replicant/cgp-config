//! The `.env` parser never panics on any input.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = cgp_config_providers::dotenv::parse_dotenv(text);
});
