//! # cgp-config-secrets
//!
//! [`Secret<T>`] and [`LazySecret<T>`], and the resolvers for `env:`, `file:`, and
//! `docker:` (design plan §6.9).
//!
//! A reference is resolved while `Secret<T>` is deserialized, from the thread-local
//! session [`LoadLayered`](cgp_config_core::LoadLayered) opens. With no resolving session
//! the reference fails with [`ConfigError::SecretsDenied`](cgp_config_core::ConfigError::SecretsDenied).
//! A plain string is a literal secret. The value tree keeps the reference (and
//! `Meta::secret`), never the resolved bytes.
//!
//! `Debug` and `Display` are `[REDACTED]`. [`serialize_redacted`] writes that word for
//! callers who opt in; `Secret<T>` does not implement `Serialize`.
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(clippy::std_instead_of_alloc, clippy::std_instead_of_core)]

extern crate alloc;

mod component;
mod lazy;
mod mock;
mod resolve;
mod roots;
mod secret;

pub use component::{
    AsyncSecretResolverComponent, CanResolveSecret, CanResolveSecretAsync, DockerScheme, EnvScheme,
    FileScheme, HasSecretRoots, SecretResolver, SecretResolverComponent, SecretRootsComponent,
    SecretRootsGetter,
};
pub use lazy::LazySecret;
pub use mock::{MockSecret, MockSecretSchemes, OpenMockSession, SecretMap};
pub use resolve::{
    DockerSecret, EnvSecret, FileSecret, StdSecretReader, StdSecretSchemes, read_within,
    resolve_docker, resolve_env, resolve_file,
};
pub use roots::SecretRoots;
pub use secret::{Secret, serialize_redacted};

/// The text used wherever a secret would otherwise be printed.
pub const REDACTED: &str = "[REDACTED]";
