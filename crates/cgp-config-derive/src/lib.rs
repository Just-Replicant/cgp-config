//! `#[derive(ConfigSection)]` for cgp-config.
//!
//! The derive expands to paths under `::cgp_config::__private`, so applications use it
//! through the facade (`cgp_config::ConfigSection`, feature `derive`). The attribute
//! reference is in the facade's `derive` module docs; the implementation lives in
//! `cgp-config-derive-impl`.

use proc_macro::TokenStream;

/// Derives `ConfigSection` (const metadata: keys, defaults, env names, parsers, nesting)
/// and `ValidateSections` (validation of nested sections, errors aggregated).
#[proc_macro_derive(ConfigSection, attributes(config))]
pub fn derive_config_section(input: TokenStream) -> TokenStream {
    cgp_config_derive_impl::derive_config_section(proc_macro2::TokenStream::from(input)).into()
}
