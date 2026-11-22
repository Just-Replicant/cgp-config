#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]
// A std crate: the workspace lints that keep the core crates `no_std`-ready do not apply.
#![allow(clippy::std_instead_of_alloc, clippy::std_instead_of_core)]

pub mod bootstrap;
#[cfg(feature = "clap")]
pub mod cli;
#[cfg(feature = "miette")]
pub mod diagnostic;
#[cfg(feature = "dotenv")]
pub mod dotenv;
pub mod env;
pub mod file;
pub mod layers;
pub mod loader;
pub mod location;
pub mod presets;
pub mod quick;
pub mod read;
#[cfg(feature = "secrets")]
pub mod secrets;
pub mod sources;
pub mod validate;
