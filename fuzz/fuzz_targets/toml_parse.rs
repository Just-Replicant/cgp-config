//! TOML parsing never panics, and whatever parses renders and parses back to the same tree.
#![no_main]

use cgp_config_core::{LoadLimits, Origin};
use cgp_config_formats::{FormatKind, parse_toml, render_document};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let origin = Origin::memory("fuzz");
    if let Ok(tree) = parse_toml(&origin, text, &LoadLimits::DEFAULT)
        && let Ok(rendered) = render_document(FormatKind::Toml, &tree)
    {
        let again = parse_toml(&origin, &rendered, &LoadLimits::DEFAULT).expect("rendered TOML parses");
        assert_eq!(again, tree);
    }
});
