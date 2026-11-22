//! Database pool section of the multi-crate example.
//!
//! Same dependency rule as `example-http-server`: `cgp-config` with `features = ["derive"]`
//! and no std IO.

#![no_std]

extern crate alloc;

use alloc::string::String;

/// Settings for one database pool.
///
/// The default document key is `db`. `#[config(getter)]` emits [`HasDbConfig`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, cgp_config::ConfigSection)]
#[config(section = "db", getter)]
pub struct DbConfig {
    /// Pool size.
    #[config(default = 5)]
    pub max_connections: u32,
    /// Database host.
    #[config(default = "localhost")]
    pub host: String,
}
