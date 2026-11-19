use alloc::string::ToString;
use alloc::vec::Vec;

use cgp_config_core::{
    ConfigError, ConfigMeta, ConfigSection, EnvSnapshot, FieldMeta, KeyPath, LoadLimits,
    NestedMeta, NestedShape,
};

use super::{Fallback, LexError, Piece, interpolate_tree, lex};
use crate::ConfigValue;

struct Db;
impl ConfigSection for Db {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "Db",
        &[
            FieldMeta::new("url", "url").with_interpolate(true),
            FieldMeta::new("password", "password").with_secret(true),
            FieldMeta::new("raw", "raw"),
        ],
    );
}

struct App;
impl ConfigSection for App {
    const META: &'static ConfigMeta = &ConfigMeta::new(
        "App",
        &[
            FieldMeta::new("host", "host").with_interpolate(true),
            FieldMeta::new("base", "base").with_interpolate(true),
            FieldMeta::new("a", "a").with_interpolate(true),
            FieldMeta::new("b", "b").with_interpolate(true),
            FieldMeta::new("plain", "plain"),
            FieldMeta::new("port", "port"),
            FieldMeta::new("db", "db").with_nested(NestedMeta::new(
                NestedShape::Single,
                <Db as ConfigSection>::meta,
            )),
            FieldMeta::new("list", "list").with_interpolate(true),
        ],
    );
}

fn env() -> EnvSnapshot {
    EnvSnapshot::from_iter([
        ("HOST", "example.org"),
        ("USER", "svc"),
        ("EVIL", "${HOST}"),
    ])
}

fn expand(tree: ConfigValue) -> Result<ConfigValue, ConfigError> {
    interpolate_tree(tree, App::META, &env(), &LoadLimits::DEFAULT)
}

fn text<'a>(tree: &'a ConfigValue, path: &str) -> &'a str {
    tree.get_path(&KeyPath::parse(path).unwrap())
        .unwrap()
        .as_str()
        .unwrap()
}

#[test]
fn lexer_forms() {
    assert_eq!(lex("plain").unwrap(), [Piece::Text("plain")]);
    assert_eq!(
        lex("$5 and $$").unwrap(),
        [Piece::Text("$5 and "), Piece::Dollar]
    );
    assert_eq!(
        lex("${A:?need A}${B:-${C:-x}}").unwrap(),
        [
            Piece::Env {
                name: "A",
                fallback: Fallback::Error("need A")
            },
            Piece::Env {
                name: "B",
                fallback: Fallback::Default("${C:-x}")
            },
        ]
    );
    assert_eq!(lex("${ref: a.b }").unwrap(), [Piece::Ref("a.b")]);
    assert_eq!(lex("x${A").unwrap_err(), LexError::Unterminated { at: 1 });
    assert_eq!(lex("${}").unwrap_err(), LexError::Empty { at: 0 });
    assert_eq!(lex("${ref:}").unwrap_err(), LexError::Empty { at: 0 });
    assert_eq!(lex("${1A}").unwrap_err(), LexError::InvalidName { at: 0 });
    assert_eq!(lex("${A-B}").unwrap_err(), LexError::InvalidName { at: 0 });
    assert_eq!(lex("").unwrap(), Vec::<Piece<'_>>::new());
}

#[test]
fn only_opted_in_strings_expand() {
    let tree = ConfigValue::map([
        ("host", ConfigValue::from("${HOST}")),
        (
            "base",
            ConfigValue::from("https://${ref:host}:${ref:port}/$${x}"),
        ),
        ("plain", ConfigValue::from("${HOST} stays")),
        ("port", ConfigValue::from(8443)),
        (
            "db",
            ConfigValue::map([
                (
                    "url",
                    ConfigValue::from("pg://${USER}@${ref:host}/${DB:-app}"),
                ),
                ("raw", ConfigValue::from("${USER}")),
            ]),
        ),
        (
            "list",
            ConfigValue::array(["${USER}".into(), "$${USER}".into()]),
        ),
        ("a", ConfigValue::from("${EVIL}")),
    ]);
    let tree = expand(tree).unwrap();
    assert_eq!(text(&tree, "host"), "example.org");
    assert_eq!(text(&tree, "base"), "https://example.org:8443/${x}");
    assert_eq!(text(&tree, "plain"), "${HOST} stays");
    assert_eq!(text(&tree, "db.url"), "pg://svc@example.org/app");
    assert_eq!(text(&tree, "db.raw"), "${USER}");
    assert_eq!(text(&tree, "list[0]"), "svc");
    assert_eq!(text(&tree, "list[1]"), "${USER}");
    // Env values are inserted literally, never re-scanned.
    assert_eq!(text(&tree, "a"), "${HOST}");
    let base = tree.get("base").unwrap();
    assert!(base.meta().expanded);
    // A second pass is a no-op: expanded nodes are not scanned again.
    let again = expand(tree.clone()).unwrap();
    assert_eq!(text(&again, "base"), "https://example.org:8443/${x}");
    assert_eq!(text(&again, "list[1]"), "${USER}");
}

#[test]
fn failures_are_reported_together() {
    let tree = ConfigValue::map([
        ("host", ConfigValue::from("${MISSING}")),
        (
            "base",
            ConfigValue::from("${OTHER:?set OTHER to the base URL}"),
        ),
        (
            "db",
            ConfigValue::map([
                ("url", ConfigValue::from("${ref:db.password}")),
                ("password", ConfigValue::from("hunter2")),
            ]),
        ),
    ]);
    let mut tree = tree;
    let db = tree.as_map_mut().unwrap().get_mut("db").unwrap();
    db.as_map_mut()
        .unwrap()
        .get_mut("password")
        .unwrap()
        .meta_mut()
        .secret = true;
    let ConfigError::Validation(errors) = expand(tree).unwrap_err() else {
        panic!("expected aggregated errors");
    };
    let messages: Vec<_> = errors.iter().map(ToString::to_string).collect();
    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages[0].contains("MISSING"));
    assert!(messages[1].contains("set OTHER to the base URL"));
    assert!(messages[2].contains("secret"));
    assert!(messages.iter().all(|m| !m.contains("hunter2")));
}

#[test]
fn cycles_and_depth() {
    let tree = ConfigValue::map([
        ("a", ConfigValue::from("${ref:b}")),
        ("b", ConfigValue::from("${ref:a}")),
    ]);
    let error = expand(tree).unwrap_err();
    assert!(
        error.to_string().contains("cycle") || error.code() == "config::validation",
        "{error}"
    );

    let deep = "${A:-${B:-${C:-${D:-x}}}}";
    let tree = ConfigValue::map([("host", ConfigValue::from(deep))]);
    let limits = LoadLimits::DEFAULT.with_max_interpolation_depth(2);
    let error = interpolate_tree(tree.clone(), App::META, &env(), &limits).unwrap_err();
    assert_eq!(error.code(), "config::limit_exceeded");
    let ok = interpolate_tree(tree, App::META, &env(), &LoadLimits::DEFAULT).unwrap();
    assert_eq!(text(&ok, "host"), "x");
}
