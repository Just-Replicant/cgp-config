use cgp_config::prelude::*;

#[derive(ConfigSection)]
enum Mode {
    A,
}

#[derive(ConfigSection)]
struct Tuple(u8);

fn main() {}
