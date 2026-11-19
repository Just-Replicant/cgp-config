use cgp_config::prelude::*;

#[derive(serde::Deserialize, ConfigSection)]
struct App {
    #[serde(rename = "$version")]
    version: u8,
    #[config(alias = "b")]
    a: u8,
    b: u8,
    #[config(skip)]
    c: u8,
    #[config(env_raw = "PORT")]
    d: u16,
    #[config(env_raw = "PORT")]
    e: u16,
}

fn main() {}
