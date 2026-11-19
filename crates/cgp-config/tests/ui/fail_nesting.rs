use cgp_config::prelude::*;

#[derive(serde::Deserialize, ConfigSection)]
struct Inner {
    x: u8,
}

#[derive(serde::Deserialize, ConfigSection)]
struct App {
    #[config(nested, default = 1)]
    a: Inner,
    #[config(flatten)]
    #[serde(flatten)]
    b: Inner,
    #[config(flatten)]
    c: Vec<Inner>,
}

fn main() {}
