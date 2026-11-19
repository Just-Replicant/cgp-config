use cgp_config::prelude::*;

struct Plain {
    x: u8,
}

#[derive(serde::Deserialize, ConfigSection)]
struct App {
    #[config(nested)]
    plain: Plain,
}

impl<'de> serde::Deserialize<'de> for Plain {
    fn deserialize<D: serde::Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Ok(Plain { x: 0 })
    }
}

fn main() {
    let _ = Plain { x: 1 }.x;
}
