//! `#[derive(ConfigSection)]` never panics: any token stream yields an impl or
//! `compile_error!`s.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    if let Ok(tokens) = text.parse::<proc_macro2::TokenStream>() {
        let _ = cgp_config_derive_impl::derive_config_section(tokens);
    }
});
