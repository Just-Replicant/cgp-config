//! # cgp-config-core
//!
//! The semver-stable vocabulary of [`cgp-config`](https://docs.rs/cgp-config): component
//! traits for every stage of the loading pipeline, key paths, provenance, the error type,
//! load limits, and the providers that do not depend on a concrete value tree.
//!
//! The crate is `#![no_std]`. Without features it offers the component traits, the
//! generic providers, [`LoadLimits`], and [`StaticKeyPath`]. The `alloc` feature adds the
//! owned types ([`KeyPath`], [`Origin`], [`Meta`], [`ConfigError`], [`LoadWarning`],
//! [`LoadReport`]) and the format traits. `std` only swaps the lock inside
//! [`LoadReport`] for `std::sync::Mutex`.
//!
//! ## The pipeline
//!
//! Every load, at startup and (from 0.2) on every reload, runs the same stages. Each stage
//! is a CGP component a context wires with `delegate_components!` or takes from a preset:
//!
//! 1. collect the sources in list order ([`CanCollectLayers`]): load
//!    ([`CanLoadSource`]), prepare ([`CanPrepareLayer`]), merge ([`CanMergeLayers`]);
//! 2. interpolate opted-in fields ([`CanInterpolate`]);
//! 3. deserialize ([`CanDeserializeConfig`]);
//! 4. resolve secrets ([`CanResolveSecrets`]);
//! 5. validate ([`CanValidateConfig`]).
//!
//! [`LoadLayered`] is the provider of [`CanLoadConfig`] that runs those stages.
//!
//! ## Wiring parametric components
//!
//! Most components here are generic over a marker or a config type. A non-generic
//! `delegate_components!` table checks itself with `()` as the parameter, which fails for
//! those components (see ADR-001). Mark such tables `#[unchecked]` and check them with
//! [`config_check!`] instead.
#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod components;
mod limits;
mod macros;
pub mod markers;
pub mod providers;
mod static_path;

#[cfg(feature = "alloc")]
mod error;
#[cfg(feature = "alloc")]
mod meta;
#[cfg(feature = "alloc")]
mod origin;
#[cfg(feature = "alloc")]
mod path;
#[cfg(feature = "alloc")]
mod report;
#[cfg(feature = "alloc")]
mod suggest;

pub use components::*;
#[cfg(feature = "alloc")]
pub use error::{ConfigError, RedactedRef, UnknownKey, ValidationErrors, ValidationIssue, codes};
pub use limits::LoadLimits;
#[cfg(feature = "alloc")]
pub use meta::{Meta, Provenance};
#[cfg(feature = "alloc")]
pub use origin::{Origin, Span};
#[cfg(feature = "alloc")]
pub use path::{KeyPath, KeyPathParseError, PathSegment};
pub use providers::*;
#[cfg(feature = "alloc")]
pub use report::{LoadReport, LoadWarning};
pub use static_path::StaticKeyPath;
#[cfg(feature = "alloc")]
pub use suggest::{edit_distance, suggest};

/// Reserved document keys. A user field cannot take these names.
pub mod reserved {
    /// Schema version of a file (`$version = 2`). Recorded and stripped in 0.1.
    pub const VERSION: &str = "$version";
    /// In-file profile tables (`[$profiles.prod]`).
    pub const PROFILES: &str = "$profiles";
    /// Root include list. Loaded from 0.2; stripped with a warning in 0.1.
    pub const INCLUDE: &str = "$include";
    /// Every reserved key.
    pub const ALL: [&str; 3] = [VERSION, PROFILES, INCLUDE];

    /// Whether `key` is reserved.
    #[must_use]
    pub fn is_reserved(key: &str) -> bool {
        ALL.contains(&key)
    }
}

/// The CGP fork, re-exported. Public signatures of this crate name the fork only
/// through this path, so a later move to upstream `cgp` replaces this module.
pub use cgp_fork as cgp;
