// Clashes that span sections are compile errors too: a key on the parent and inside a
// flattened section, two flattened sections sharing a key, and an `env_raw` name used
// twice under one root.
use cgp_config::ConfigSection;
use serde::Deserialize;

#[derive(Deserialize, ConfigSection)]
struct Common {
    log_level: String,
}

#[derive(Deserialize, ConfigSection)]
struct Parent {
    log_level: String,
    #[config(flatten)]
    common: Common,
}

#[derive(Deserialize, ConfigSection)]
struct Logging {
    #[config(alias = "LOG_LEVEL")]
    level: String,
}

#[derive(Deserialize, ConfigSection)]
struct TwoFlattened {
    #[config(flatten)]
    common: Common,
    #[config(flatten)]
    logging: Logging,
}

#[derive(Deserialize, ConfigSection)]
struct Http {
    #[config(env_raw = "PORT")]
    port: u16,
}

#[derive(Deserialize, ConfigSection)]
struct RawTwice {
    #[config(env_raw = "PORT")]
    admin_port: u16,
    #[config(nested)]
    http: Http,
}

fn main() {}
