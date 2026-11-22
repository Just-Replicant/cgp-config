//! The redacted stamp of a tree: canonical JSON, fingerprint, and `explain` nodes.
//!
//! Built after schema normalization, so secret fields are already marked. A secret node
//! becomes the JSON string `"[REDACTED]"` and is not walked. A `{ secret = "scheme:rest" }`
//! table also records the redacted reference, never the resolved secret and never an
//! unsafe path.

use alloc::string::String;
use alloc::vec::Vec;

use cgp_config_core::{ExplainNode, KeyPath, LoadStamp, SecretRef};

use crate::REDACTED;
use crate::value::{ConfigValue, Kind};

/// The stamp `SerdeWithProvenance` records on the load report.
pub(crate) fn stamp(value: &ConfigValue) -> LoadStamp {
    let mut nodes = Vec::new();
    walk(value, KeyPath::root(), &mut nodes);
    LoadStamp::new(value.meta().origin.clone(), nodes, canonical(value))
}

fn walk(value: &ConfigValue, path: KeyPath, nodes: &mut Vec<ExplainNode>) {
    let reference = redacted_reference(value);
    let secret = value.meta().secret || reference.is_some();
    nodes.push(ExplainNode::new(
        path.clone(),
        if secret {
            String::from(REDACTED)
        } else {
            preview(value)
        },
        value.meta().origin.clone(),
        value.meta().span,
        value.meta().shadowed.clone(),
        secret,
        reference,
    ));
    if secret {
        return;
    }
    match value.kind() {
        Kind::Array(elements) => {
            for (index, element) in elements.iter().enumerate() {
                walk(element, path.join(index), nodes);
            }
        }
        Kind::Map(map) => {
            for (key, child) in map.iter_shared() {
                walk(child, path.join(key.as_ref()), nodes);
            }
        }
        _ => {}
    }
}

/// The redacted `scheme:rest` of a reference table, when the `secret` key parses as one.
fn redacted_reference(value: &ConfigValue) -> Option<String> {
    let Kind::Map(map) = value.kind() else {
        return None;
    };
    let text = map.get("secret")?.as_str()?;
    let reference = SecretRef::parse(text)?;
    Some(alloc::format!("{}", reference.redacted()))
}

fn preview(value: &ConfigValue) -> String {
    match value.kind() {
        Kind::Null => String::from("null"),
        Kind::Bool(flag) => String::from(if *flag { "true" } else { "false" }),
        Kind::Number(number) => canonical_number(number),
        Kind::String(text) => json_string(text),
        Kind::Array(elements) => alloc::format!("array[{}]", elements.len()),
        Kind::Map(map) => alloc::format!("table[{}]", map.len()),
    }
}

/// Canonical JSON: no extra whitespace, map keys sorted, secrets replaced.
fn canonical(value: &ConfigValue) -> String {
    if value.meta().secret {
        return json_string(REDACTED);
    }
    match value.kind() {
        Kind::Null => String::from("null"),
        Kind::Bool(true) => String::from("true"),
        Kind::Bool(false) => String::from("false"),
        Kind::Number(number) => canonical_number(number),
        Kind::String(text) => json_string(text),
        Kind::Array(elements) => {
            let mut out = String::from("[");
            for (index, element) in elements.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&canonical(element));
            }
            out.push(']');
            out
        }
        Kind::Map(map) => {
            let mut keys: Vec<&str> = map.keys().collect();
            keys.sort_unstable();
            let mut out = String::from("{");
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&json_string(key));
                out.push(':');
                match map.get(key) {
                    Some(child) => out.push_str(&canonical(child)),
                    None => out.push_str("null"),
                }
            }
            out.push('}');
            out
        }
    }
}

fn canonical_number(number: &crate::Number) -> String {
    if let Some(value) = number.as_u64() {
        return alloc::format!("{value}");
    }
    if let Some(value) = number.as_i64() {
        return alloc::format!("{value}");
    }
    let value = number.as_f64();
    serde_json::to_string(&value).unwrap_or_else(|_| String::from("null"))
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| String::from("\"\""))
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use cgp_config_core::{Meta, Origin};

    use super::stamp;
    use crate::ConfigValue;

    fn secret_string(text: &str) -> ConfigValue {
        ConfigValue::from(text).with_meta(Meta::default().with_secret(true))
    }

    #[test]
    fn fingerprint_ignores_secret_bytes_and_sorts_keys() {
        let left = ConfigValue::map([
            ("password", secret_string("s3cr3t-SENTINEL")),
            ("name", ConfigValue::from("app")),
        ]);
        let right = ConfigValue::map([
            ("name", ConfigValue::from("app")),
            ("password", secret_string("other-value")),
        ]);
        let left = stamp(&left);
        let right = stamp(&right);
        assert_eq!(left.fingerprint, right.fingerprint);
        assert_eq!(left.dump_json, r#"{"name":"app","password":"[REDACTED]"}"#);
        assert!(!left.dump_json.contains("SENTINEL"));
        let shown = left.explain("password").unwrap().to_string();
        assert!(shown.contains("[REDACTED]"), "{shown}");
        assert!(!shown.contains("SENTINEL"), "{shown}");
    }

    #[test]
    fn a_reference_table_explains_the_redacted_name_only() {
        let tree = ConfigValue::map([(
            "password",
            ConfigValue::map([("secret", ConfigValue::from("env:DB_PASSWORD"))])
                .with_meta(Meta::new(Origin::file("app.toml")).with_secret(true)),
        )]);
        let recorded = stamp(&tree);
        let shown = recorded.explain("password").unwrap().to_string();
        assert!(shown.contains("[REDACTED]"), "{shown}");
        assert!(shown.contains("env:DB_PASSWORD"), "{shown}");
        assert!(shown.contains("app.toml"), "{shown}");
        assert!(
            !recorded.dump_json.contains("DB_PASSWORD"),
            "{}",
            recorded.dump_json
        );
        let climbed = ConfigValue::map([("secret", ConfigValue::from("file:../secret"))]);
        let shown = stamp(&climbed).explain("(root)").unwrap().to_string();
        assert!(shown.contains("file:<redacted #"), "{shown}");
        assert!(!shown.contains(".."), "{shown}");
    }
}
