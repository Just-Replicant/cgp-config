use cgp_config::prelude::*;

#[derive(serde::Deserialize, ConfigSection)]
#[config(migrate(from = 1))]
#[config(sectoin = "typo")]
struct App {
    #[config(default)]
    a: u16,
    #[config(parse = "duration", default = 30)]
    b: std::time::Duration,
    #[config(env = "A__B")]
    c: u8,
    #[config(reload = "sometimes")]
    d: u8,
}

fn main() {}
