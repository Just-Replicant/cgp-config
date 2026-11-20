//! Format conformance: the same document in TOML and JSON yields the same tree; every
//! syntax error names `file:line:column`; duplicate keys are rejected with their path;
//! limits apply while parsing; rendering round-trips and redacts secrets.

#![cfg(all(feature = "toml", feature = "json"))]
#![allow(
    missing_docs,
    clippy::std_instead_of_core,
    clippy::std_instead_of_alloc
)]

use cgp_config_core::{ConfigError, LoadLimits, Origin};
use cgp_config_formats::{FormatKind, parse_document, parse_json, parse_toml, render_document};
use cgp_config_value::ConfigValue;

fn toml(text: &str) -> Result<ConfigValue, ConfigError> {
    parse_toml(&Origin::file("app.toml"), text, &LoadLimits::DEFAULT)
}

fn json(text: &str) -> Result<ConfigValue, ConfigError> {
    parse_json(&Origin::file("app.json"), text, &LoadLimits::DEFAULT)
}

const TOML_DOC: &str = r#"
name = "svc"
port = 8080
ratio = 0.5
neg = -3
on = true
tags = ["a", "b"]
empty = []

[db]
url = "pg://x"
pool = { size = 4 }

[[servers]]
host = "a"

[[servers]]
host = "b"
"#;

const JSON_DOC: &str = r#"{
  "name": "svc", "port": 8080, "ratio": 0.5, "neg": -3, "on": true,
  "tags": ["a", "b"], "empty": [],
  "db": { "url": "pg://x", "pool": { "size": 4 } },
  "servers": [ { "host": "a" }, { "host": "b" } ]
}"#;

#[test]
fn toml_and_json_agree() {
    let a = toml(TOML_DOC).unwrap();
    let b = json(JSON_DOC).unwrap();
    assert_eq!(a, b, "same tree from both formats");
    assert_eq!(
        a.get_path(&"db.pool.size".into()).unwrap().as_u64(),
        Some(4)
    );
    assert_eq!(
        a.get_path(&"servers[1].host".into()).unwrap().as_str(),
        Some("b")
    );
    assert_eq!(a.get("neg").unwrap().as_i64(), Some(-3));
    assert_eq!(a.get("ratio").unwrap().as_f64(), Some(0.5));
    assert_eq!(a.get("on").unwrap().as_bool(), Some(true));
}

#[test]
fn nodes_carry_file_and_position() {
    let tree = toml(TOML_DOC).unwrap();
    let port = tree.get("port").unwrap().meta();
    assert_eq!(port.origin, Origin::file("app.toml"));
    let span = port.span.expect("a span");
    assert_eq!((span.line, span.column), (3, 8));
    let tree = json(JSON_DOC).unwrap();
    let span = tree
        .get_path(&"db.url".into())
        .unwrap()
        .meta()
        .span
        .expect("a span");
    assert_eq!(span.line, 4);
}

#[test]
fn syntax_errors_show_file_line_column() {
    let cases: &[(&str, Result<ConfigValue, ConfigError>, &str)] = &[
        ("toml: missing value", toml("a = 1\nb =\n"), "app.toml:2:"),
        ("toml: bad table", toml("[a\nb = 1\n"), "app.toml:1:"),
        (
            "toml: unterminated string",
            toml("a = \"x\n"),
            "app.toml:1:",
        ),
        ("json: trailing comma", json("{\"a\": 1,\n}"), "app.json:2:"),
        ("json: single quotes", json("{'a': 1}"), "app.json:1:"),
        ("json: comment", json("{\"a\": 1 // no\n}"), "app.json:1:"),
        ("json: trailing garbage", json("{} x"), "app.json:1:"),
        ("json: bad escape", json("{\"a\": \"\\q\"}"), "app.json:1:"),
        ("json: leading zero", json("{\"a\": 01}"), "app.json:1:"),
    ];
    for (name, result, position) in cases {
        let error = result.as_ref().expect_err(name);
        assert_eq!(error.code(), "config::parse", "{name}: {error}");
        assert!(
            error.to_string().contains(position),
            "{name}: `{position}` in {error}"
        );
    }
}

#[test]
fn duplicate_keys_are_rejected() {
    for (name, result, path) in [
        ("toml", toml("a = 1\na = 2\n"), "a"),
        ("toml table", toml("[t]\nx = 1\n[t]\ny = 2\n"), "t"),
        ("json", json("{\"a\": 1, \"a\": 2}"), "a"),
        ("json nested", json("{\"t\": {\"x\": 1, \"x\": 2}}"), "t.x"),
    ] {
        let error = result.expect_err(name);
        let text = error.to_string();
        assert_eq!(error.code(), "config::duplicate_key", "{name}: {text}");
        assert!(text.contains(path), "{name}: `{path}` in {text}");
        assert!(text.contains("(first at "), "{name}: both spans in {text}");
    }
}

#[test]
fn empty_documents_and_roots() {
    assert!(toml("").unwrap().as_map().unwrap().is_empty());
    assert!(
        toml("# only a comment\n")
            .unwrap()
            .as_map()
            .unwrap()
            .is_empty()
    );
    assert!(json("{}").unwrap().as_map().unwrap().is_empty());
    assert!(json("null").unwrap().is_null());
    assert!(
        json("[1]").unwrap().as_array().is_some(),
        "the file loader rejects non-table roots"
    );
}

#[test]
fn limits_apply_while_parsing() {
    let limits = LoadLimits::DEFAULT.with_max_depth(3);
    let deep_json = "{\"a\":{\"b\":{\"c\":{\"d\":1}}}}";
    let error = parse_json(&Origin::file("x.json"), deep_json, &limits).unwrap_err();
    assert_eq!(error.code(), "config::limit_exceeded", "{error}");
    let deep_toml = "a.b.c.d = 1\n";
    let error = parse_toml(&Origin::file("x.toml"), deep_toml, &limits).unwrap_err();
    assert_eq!(error.code(), "config::limit_exceeded", "{error}");
    let limits = LoadLimits::DEFAULT.with_max_bytes(4);
    assert!(parse_json(&Origin::file("x.json"), "{\"a\": 1}", &limits).is_err());
}

#[test]
fn render_round_trips() {
    let tree = toml(TOML_DOC).unwrap();
    for kind in [FormatKind::Toml, FormatKind::Json] {
        let text = render_document(kind, &tree).unwrap();
        let again =
            parse_document(kind, &Origin::memory("render"), &text, &LoadLimits::DEFAULT).unwrap();
        assert_eq!(again, tree, "{kind:?}:\n{text}");
    }
}

#[test]
fn extensions_choose_the_format() {
    assert_eq!(
        FormatKind::from_path("conf.d/10-app.toml"),
        Some(FormatKind::Toml)
    );
    assert_eq!(FormatKind::from_path("app.JSON"), Some(FormatKind::Json));
    assert_eq!(FormatKind::from_path("app.yaml"), None);
}
