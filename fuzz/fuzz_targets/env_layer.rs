//! Mapping `{PREFIX}__A__B` variables into a layer never panics; conflicts are errors.
#![no_main]

use cgp_config_core::EnvSnapshot;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|pairs: Vec<(String, String)>| {
    let env: EnvSnapshot = pairs.into_iter().collect();
    let _ = cgp_config_providers::env::env_layer(&env, "APP", None);
});
