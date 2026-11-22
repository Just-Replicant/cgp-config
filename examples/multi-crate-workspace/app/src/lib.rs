//! Application side of the multi-crate example.
//!
//! Mounts [`example_http_server::HttpConfig`] and [`example_db_pool::DbConfig`], loads them
//! with [`cgp_config::quick::load_from`], and projects each section out of an `Arc` with
//! [`ProjectSection`](cgp_config::ProjectSection).

#![allow(clippy::std_instead_of_alloc, clippy::std_instead_of_core)]

use std::sync::Arc;

use cgp_config::prelude::*;
use example_db_pool::{DbConfig, DbConfigGetterComponent};
use example_http_server::{HttpConfig, HttpConfigGetterComponent};

/// The process configuration. Each library section is mounted under its own key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, ConfigSection, HasField)]
pub struct AppConfig {
    /// The HTTP server, mounted at `http` (env `MYAPP__HTTP__...`).
    #[config(nested, mount = "http")]
    pub http: HttpConfig,
    /// The database pool, mounted at `db` (env `MYAPP__DB__...`).
    #[config(nested, mount = "db")]
    pub db: DbConfig,
    /// Process name.
    #[config(default = "demo")]
    pub name: String,
}

/// A request that holds the config for the length of the request.
///
/// The long-lived process would store a handle and copy an `Arc<AppConfig>` into this
/// struct at the start of the request. The getters borrow from that `Arc`.
#[derive(HasField)]
pub struct Request {
    /// The config for this request.
    pub config: Arc<AppConfig>,
}

delegate_components! {
    Request {
        HttpConfigGetterComponent: WithProvider<ProjectSection<Path!(@config.http)>>,
        DbConfigGetterComponent: WithProvider<ProjectSection<Path!(@config.db)>>,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cgp_config::prelude::*;
    use example_db_pool::HasDbConfig;
    use example_http_server::HasHttpConfig;

    use super::{AppConfig, Request};

    #[test]
    fn wires_two_sections_from_a_virtual_load() {
        let (config, report): (AppConfig, _) = cgp_config::quick::load_from(
            "myapp",
            EnvSnapshot::from_iter([
                ("MYAPP__HTTP__PORT", "7000"),
                ("MYAPP__DB__MAX_CONNECTIONS", "12"),
            ]),
            [(
                "myapp.toml",
                "\"$version\" = 1\nname = \"demo\"\n[db]\nhost = \"db.internal\"\n",
            )],
        )
        .expect("load");
        assert_eq!(config.name, "demo");
        assert_eq!(config.http.port, 7000);
        assert_eq!(config.http.host, "0.0.0.0");
        assert_eq!(config.db.max_connections, 12);
        assert_eq!(config.db.host, "db.internal");
        assert!(
            report.warnings().iter().all(|warning| {
                matches!(warning, LoadWarning::DocumentVersion { version: 1, .. })
            }),
            "{report:?}"
        );

        let request = Request {
            config: Arc::new(config),
        };
        assert_eq!(request.http_config().port, 7000);
        assert_eq!(request.db_config().host, "db.internal");
    }

    #[test]
    fn the_same_section_can_be_mounted_twice() {
        #[derive(Debug, serde::Deserialize, ConfigSection, PartialEq)]
        struct Two {
            #[config(nested, mount = "api_http")]
            api: example_http_server::HttpConfig,
            #[config(nested, mount = "admin_http")]
            admin: example_http_server::HttpConfig,
        }

        let config: Two = cgp_config::from_toml_str(
            "[api_http]\nport = 1\n[admin_http]\nport = 2\nhost = \"127.0.0.1\"\n",
        )
        .expect("toml");
        assert_eq!(config.api.port, 1);
        assert_eq!(config.admin.port, 2);
        assert_eq!(config.admin.host, "127.0.0.1");
        assert_eq!(config.api.host, "0.0.0.0");
    }
}
