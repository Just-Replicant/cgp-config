use cgp_config::prelude::*;

#[derive(serde::Deserialize, ConfigSection)]
struct Inner {
    #[config(default = true)]
    enabled: bool,
}

#[derive(serde::Deserialize, ConfigSection)]
#[serde(rename_all = "kebab-case")]
#[config(section = "app", version = 1, env_prefix = "APP", deny_unknown)]
struct App {
    /// Docs become metadata.
    #[config(default = 1, env = "N", alias = "count", deprecated = "use n")]
    n: u32,
    #[config(default = -2)]
    offset: i64,
    #[config(default = [1.5, 2.0])]
    weights: Vec<f64>,
    #[config(secret, reload = "restart", merge = "replace", interpolate)]
    token: Option<String>,
    #[config(default = "1h", parse = "duration")]
    ttl: std::time::Duration,
    #[config(default = 1024, parse = "bytes")]
    buffer: u64,
    #[config(nested)]
    inner: Inner,
    #[config(nested)]
    maybe: Option<Inner>,
    #[config(nested)]
    #[serde(default)]
    many: Vec<Inner>,
    #[config(flatten)]
    flat: Inner,
    #[config(mount = "renamed")]
    original: Option<u8>,
    #[serde(skip)]
    #[config(skip)]
    runtime: (),
}

fn main() {
    assert_eq!(App::META.section, Some("app"));
    assert_eq!(App::META.fields.len(), 11);
}
