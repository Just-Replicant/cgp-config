//! `KeyPath::parse` reads back whatever `Display` writes, for every key text.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use cgp_config_core::{KeyPath, PathSegment};
use proptest::prelude::*;

fn segment() -> impl Strategy<Value = PathSegment> {
    prop_oneof![
        // Keys that collide with path syntax, including the root's own text.
        prop_oneof![
            Just(String::from("(root)")),
            Just(String::new()),
            Just(String::from("a.b")),
            Just(String::from("[0]")),
            Just(String::from("q\"uote\\")),
            "[ -~]{0,6}",
            "\\PC{0,4}",
        ]
        .prop_map(|key| PathSegment::from(key.as_str())),
        (0usize..1000).prop_map(PathSegment::Index),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(if cfg!(miri) { 4 } else { 512 }))]

    #[test]
    fn display_then_parse_is_the_identity(segments in prop::collection::vec(segment(), 0..5)) {
        let path = KeyPath::from_segments(segments);
        let text = path.to_string();
        prop_assert_eq!(KeyPath::parse(&text).unwrap(), path.clone(), "{}", text);
        // Only the root prints as the bare `(root)`.
        prop_assert_eq!(text == "(root)", path.is_root());
    }
}
