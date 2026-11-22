use cgp_config::prelude::*;

#[derive(serde::Deserialize, ConfigSection)]
struct Http {
    port: u16,
}

#[derive(serde::Deserialize, ConfigSection)]
struct App {
    #[config(mount = "http")]
    left: Http,
    #[config(mount = "http")]
    right: Http,
}

fn main() {}
