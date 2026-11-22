//! HTTP section of the multi-crate example.
//!
//! This crate depends on `cgp-config` with `default-features = false, features = ["derive"]`.
//! It is `no_std`: it does not read the process environment or the filesystem. The
//! application mounts the section and loads it.

#![no_std]

extern crate alloc;

use alloc::string::String;

/// Settings for one HTTP server.
///
/// The default document key is `http`. An application can mount it elsewhere with
/// `#[config(mount = "...")]`. `#[config(getter)]` emits [`HasHttpConfig`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, cgp_config::ConfigSection)]
#[config(section = "http", getter)]
pub struct HttpConfig {
    /// Port to bind.
    #[config(default = 8080)]
    pub port: u16,
    /// Address to bind.
    #[config(default = "0.0.0.0")]
    pub host: String,
}

#[cfg(test)]
mod tests {
    use cgp_config::ConfigSection;

    use super::HttpConfig;

    #[test]
    fn section_key_and_port_default() {
        assert_eq!(HttpConfig::META.section, Some("http"));
        assert_eq!(HttpConfig::META.field("port").unwrap().doc_key, "port");
        assert!(HttpConfig::META.field("host").unwrap().default.is_some());
    }
}
