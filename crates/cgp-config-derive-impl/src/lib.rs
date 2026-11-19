//! The implementation of `#[derive(ConfigSection)]` (see the crate README).
//!
//! [`derive_config_section`] turns a struct into:
//!
//! * `impl ConfigSection for T` with a `const META: &'static ConfigMeta` built from the
//!   struct's `#[config(...)]` and `#[serde(...)]` attributes and doc comments;
//! * `impl<Context> ValidateSections<Context> for T`, which validates every nested
//!   section with the context's own validators and aggregates their errors.
//!
//! Misuse is a compile error at the offending attribute; all errors of one struct are
//! reported together.

mod attrs;
mod case;
mod emit;
mod model;
mod ty;

use proc_macro2::TokenStream;

/// Expands `#[derive(ConfigSection)]` on `input`. Never panics: invalid input becomes
/// `compile_error!` invocations.
#[must_use]
pub fn derive_config_section(input: TokenStream) -> TokenStream {
    let input = match syn::parse2::<syn::DeriveInput>(input) {
        Ok(input) => input,
        Err(error) => return error.to_compile_error(),
    };
    match model::Section::from_input(&input) {
        Ok(section) => emit::expand(&section),
        Err(error) => error.to_compile_error(),
    }
}

#[cfg(test)]
mod tests;
