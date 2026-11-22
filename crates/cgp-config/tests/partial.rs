//! `#[config(partial)]` fills defaults and reports every missing field.

#![cfg(all(feature = "std", feature = "derive"))]
#![allow(missing_docs)]

use core::time::Duration;

use cgp_config::prelude::*;

#[derive(Debug, PartialEq, serde::Deserialize, ConfigSection)]
#[config(partial)]
struct HttpConfig {
    #[config(default = 8080)]
    port: u16,
    host: String,
    #[config(default = "30s", parse = "duration")]
    timeout: Duration,
    #[serde(skip)]
    skipped: u8,
}

#[test]
fn missing_fields_are_reported_together() {
    let error = HttpConfigPartial {
        port: None,
        host: None,
        timeout: None,
    }
    .finalize_optional_all()
    .expect_err("host is required");
    let names: Vec<_> = error.names().collect();
    assert_eq!(names, ["host"]);
    assert!(!names.contains(&"port"), "a const default is not missing");
}

#[test]
fn present_values_win_over_defaults() {
    let http = HttpConfigPartial {
        port: Some(9),
        host: Some("h".to_owned()),
        timeout: Some(Duration::from_secs(1)),
    }
    .finalize_optional_all()
    .expect("complete");
    assert_eq!(
        http,
        HttpConfig {
            port: 9,
            host: "h".to_owned(),
            timeout: Duration::from_secs(1),
            skipped: 0,
        }
    );
}
