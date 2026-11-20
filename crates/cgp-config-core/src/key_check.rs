//! The compile-time uniqueness check of section metadata: [`ConfigMeta::checked`].
//!
//! `#[derive(ConfigSection)]` wraps every generated `META` in [`ConfigMeta::checked`], so
//! a clash is a compile error ("evaluation panicked: cgp-config: ...") at the type that
//! causes it, instead of a key that silently goes to the wrong field at run time.
//!
//! The check walks the section, the sections it flattens, and (for `env_raw`) every
//! single nested section, through [`NestedMeta::section_ref`](crate::NestedMeta). It
//! needs no allocator: names are compared byte by byte and the panic message is built in
//! a fixed buffer.

use crate::section::{ConfigMeta, FieldMeta, NestedShape};

/// Levels of flattened / nested sections followed (the same bound as `FieldChain`).
const MAX_DEPTH: usize = crate::FieldChain::CAPACITY;

/// A field that answers on the level being checked, with the section that declares it.
#[derive(Clone, Copy)]
struct Visible {
    field: &'static FieldMeta,
    owner: &'static ConfigMeta,
}

/// The section a field flattens, when the compile-time walk can follow it.
const fn flattened_section(field: &FieldMeta) -> Option<&'static ConfigMeta> {
    if !field.flatten {
        return None;
    }
    single_section_ref(field)
}

const fn single_section_ref(field: &FieldMeta) -> Option<&'static ConfigMeta> {
    match field.nested {
        Some(nested) if matches!(nested.shape, NestedShape::Single) => nested.section_ref,
        _ => None,
    }
}

/// The `n`th field visible on `meta` (its own fields, then the fields of flattened
/// sections in place), or the count still to skip.
const fn nth_visible(
    meta: &'static ConfigMeta,
    mut n: usize,
    depth: usize,
) -> (Option<Visible>, usize) {
    let mut index = 0;
    while index < meta.fields.len() {
        let field = &meta.fields[index];
        index += 1;
        if field.flatten {
            if let Some(section) = flattened_section(field)
                && depth + 1 < MAX_DEPTH
            {
                let (found, rest) = nth_visible(section, n, depth + 1);
                if found.is_some() {
                    return (found, 0);
                }
                n = rest;
            }
            continue;
        }
        if n == 0 {
            return (Some(Visible { field, owner: meta }), 0);
        }
        n -= 1;
    }
    (None, n)
}

const fn visible(meta: &'static ConfigMeta, n: usize) -> Option<Visible> {
    nth_visible(meta, n, 0).0
}

/// The `n`th field with `env_raw` in `meta` and every single nested section below it.
const fn nth_raw(
    meta: &'static ConfigMeta,
    mut n: usize,
    depth: usize,
) -> (Option<Visible>, usize) {
    let mut index = 0;
    while index < meta.fields.len() {
        let field = &meta.fields[index];
        index += 1;
        if field.env_raw.is_some() {
            if n == 0 {
                return (Some(Visible { field, owner: meta }), 0);
            }
            n -= 1;
        }
        if let Some(section) = single_section_ref(field)
            && depth + 1 < MAX_DEPTH
        {
            let (found, rest) = nth_raw(section, n, depth + 1);
            if found.is_some() {
                return (found, 0);
            }
            n = rest;
        }
    }
    (None, n)
}

/// ASCII case-insensitive equality, `-` and `_` equal (how env segments match keys).
const fn eq_loose(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut index = 0;
    while index < a.len() {
        if dash(a[index].to_ascii_lowercase()) != dash(b[index].to_ascii_lowercase()) {
            return false;
        }
        index += 1;
    }
    true
}

const fn dash(byte: u8) -> u8 {
    if byte == b'-' { b'_' } else { byte }
}

/// ASCII case-insensitive equality.
const fn eq_ignore_case(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut index = 0;
    while index < a.len() {
        if !a[index].eq_ignore_ascii_case(&b[index]) {
            return false;
        }
        index += 1;
    }
    true
}

const fn eq_exact(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut index = 0;
    while index < a.len() {
        if a[index] != b[index] {
            return false;
        }
        index += 1;
    }
    true
}

const SNAKE_CAPACITY: usize = 256;

/// The snake-case fold of `text` (see `cgp_config_core::env_fold` for the rule), into a
/// fixed buffer. Returns the length, or `None` when the text does not fit.
const fn snake_into(text: &str, out: &mut [u8; SNAKE_CAPACITY]) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut len = 0;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if crate::env_fold::starts_word(bytes, index) {
            if len >= SNAKE_CAPACITY {
                return None;
            }
            out[len] = b'_';
            len += 1;
        }
        if len >= SNAKE_CAPACITY {
            return None;
        }
        out[len] = dash(byte.to_ascii_lowercase());
        len += 1;
        index += 1;
    }
    Some(len)
}

/// Whether two env names select the same field: loosely equal, or equal once both fold
/// to snake case (`MaxConnections`, `max_connections`, `max-connections`).
const fn env_eq(a: &str, b: &str) -> bool {
    if eq_loose(a, b) {
        return true;
    }
    let mut left = [0u8; SNAKE_CAPACITY];
    let mut right = [0u8; SNAKE_CAPACITY];
    let (Some(left_len), Some(right_len)) = (snake_into(a, &mut left), snake_into(b, &mut right))
    else {
        return false;
    };
    if left_len != right_len {
        return false;
    }
    let mut index = 0;
    while index < left_len {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// The env segment that selects a field: its `env` name, or its document key.
const fn env_name(field: &FieldMeta) -> &'static str {
    match field.env {
        Some(env) => env,
        None => field.doc_key,
    }
}

/// The `n`th name a field answers to in documents: its document key, then its aliases.
const fn key_name(field: &FieldMeta, n: usize) -> Option<&'static str> {
    if n == 0 {
        Some(field.doc_key)
    } else if n - 1 < field.aliases.len() {
        Some(field.aliases[n - 1])
    } else {
        None
    }
}

/// A panic message built without an allocator.
struct Message {
    buf: [u8; 512],
    len: usize,
}

impl Message {
    const fn new() -> Self {
        Self {
            buf: [0; 512],
            len: 0,
        }
    }

    const fn push(&mut self, text: &str) -> &mut Self {
        let bytes = text.as_bytes();
        let mut index = 0;
        // Stop at a char boundary so the buffer stays UTF-8.
        let mut room = self.buf.len() - self.len;
        if bytes.len() < room {
            room = bytes.len();
        } else {
            while room > 0 && room < bytes.len() && (bytes[room] & 0xC0) == 0x80 {
                room -= 1;
            }
        }
        while index < room {
            self.buf[self.len] = bytes[index];
            self.len += 1;
            index += 1;
        }
        self
    }

    const fn field(&mut self, item: Visible) -> &mut Self {
        self.push("`")
            .push(item.owner.name)
            .push(".")
            .push(item.field.name)
            .push("`")
    }

    const fn fail(&self) -> ! {
        let (text, _) = self.buf.split_at(self.len);
        match core::str::from_utf8(text) {
            Ok(text) => panic!("{}", text),
            Err(_) => panic!("cgp-config: conflicting keys in a config section"),
        }
    }
}

const fn clash(kind: &str, name: &str, first: Visible, second: Visible, rule: &str) -> ! {
    let mut message = Message::new();
    message
        .push("cgp-config: ")
        .field(first)
        .push(" and ")
        .field(second)
        .push(" both use the ")
        .push(kind)
        .push(" `")
        .push(name)
        .push("` (")
        .push(rule)
        .push(")");
    message.fail()
}

impl ConfigMeta {
    /// Checks `meta` and returns it; panics (a compile error in a `const`) on a clash.
    ///
    /// Clashes, each named with both fields:
    ///
    /// * two fields that answer to one document key or alias (ASCII case-insensitive),
    ///   counting the fields of flattened sections as fields of the parent;
    /// * two such fields that select the same env segment (`env` name or document key,
    ///   compared as env names fold: case, `-`/`_`, and snake case);
    /// * two fields of one struct with the same serde key;
    /// * two fields anywhere under the section (single nested sections included) that
    ///   read the same `env_raw` variable (ASCII case-insensitive).
    ///
    /// `#[derive(ConfigSection)]` calls it for every type; hand-written metadata may call
    /// it the same way:
    ///
    /// ```
    /// use cgp_config_core::{ConfigMeta, FieldMeta};
    ///
    /// const META: &ConfigMeta = ConfigMeta::checked(&ConfigMeta::new(
    ///     "Http",
    ///     &[FieldMeta::new("port", "port"), FieldMeta::new("host", "host")],
    /// ));
    /// assert_eq!(META.fields.len(), 2);
    /// ```
    ///
    /// ```compile_fail
    /// use cgp_config_core::{ConfigMeta, FieldMeta};
    ///
    /// const META: &ConfigMeta = ConfigMeta::checked(&ConfigMeta::new(
    ///     "Http",
    ///     &[FieldMeta::new("port", "port"), FieldMeta::new("listen", "listen").with_aliases(&["PORT"])],
    /// ));
    /// # fn main() { let _ = META; }
    /// ```
    ///
    /// Sections reached only through a function pointer (lists, maps, and hand-written
    /// [`NestedMeta`](crate::NestedMeta) without `section_ref`) are not walked here; the
    /// load reports their runtime clashes (`ConfigError::DuplicateKey`,
    /// `ConfigError::AmbiguousEnv`).
    ///
    /// # Panics
    ///
    /// On the first clash found.
    #[must_use]
    pub const fn checked(meta: &'static ConfigMeta) -> &'static ConfigMeta {
        check_serde_keys(meta);
        check_visible(meta);
        check_raw(meta);
        meta
    }
}

const fn check_serde_keys(meta: &'static ConfigMeta) {
    let mut first = 0;
    while first < meta.fields.len() {
        let mut second = first + 1;
        while second < meta.fields.len() {
            let (a, b) = (&meta.fields[first], &meta.fields[second]);
            if eq_exact(a.key, b.key) {
                clash(
                    "serde key",
                    a.key,
                    Visible {
                        field: a,
                        owner: meta,
                    },
                    Visible {
                        field: b,
                        owner: meta,
                    },
                    "serde keys of one struct must differ",
                );
            }
            second += 1;
        }
        first += 1;
    }
}

const KEY_RULE: &str = "keys and aliases must differ, ignoring ASCII case, across a section and the sections it flattens";
const ENV_RULE: &str =
    "env names fold case, `-`, and snake case, so they must differ after folding";

const fn check_visible(meta: &'static ConfigMeta) {
    let mut first = 0;
    while let Some(a) = visible(meta, first) {
        // A field's own names must differ too (`alias` equal to the key).
        check_names(a, a, true);
        let mut second = first + 1;
        while let Some(b) = visible(meta, second) {
            check_names(a, b, false);
            if env_eq(env_name(a.field), env_name(b.field)) {
                clash("env segment", env_name(a.field), a, b, ENV_RULE);
            }
            second += 1;
        }
        first += 1;
    }
}

/// Compares every document name of `a` with every one of `b` (or, for one field, its
/// names with each other).
const fn check_names(a: Visible, b: Visible, same: bool) {
    let mut left = 0;
    while let Some(left_name) = key_name(a.field, left) {
        let mut right = if same { left + 1 } else { 0 };
        while let Some(right_name) = key_name(b.field, right) {
            if eq_ignore_case(left_name, right_name) {
                clash("key", left_name, a, b, KEY_RULE);
            }
            right += 1;
        }
        left += 1;
    }
}

const fn check_raw(meta: &'static ConfigMeta) {
    let mut first = 0;
    while let (Some(a), _) = nth_raw(meta, first, 0) {
        let mut second = first + 1;
        while let (Some(b), _) = nth_raw(meta, second, 0) {
            if let (Some(left), Some(right)) = (a.field.env_raw, b.field.env_raw)
                && eq_ignore_case(left, right)
            {
                clash(
                    "env_raw variable",
                    left,
                    a,
                    b,
                    "an env_raw name is process-global, so it must be unique under the root",
                );
            }
            second += 1;
        }
        first += 1;
    }
}

#[cfg(test)]
mod tests {
    use crate::{ConfigMeta, ConfigSection, FieldMeta, NestedMeta, NestedShape};

    struct Common;
    impl ConfigSection for Common {
        const META: &'static ConfigMeta = ConfigMeta::checked(&ConfigMeta::new(
            "Common",
            &[FieldMeta::new("log_level", "log_level").with_env_raw("LOG_LEVEL")],
        ));
    }

    struct Http;
    impl ConfigSection for Http {
        const META: &'static ConfigMeta = ConfigMeta::checked(&ConfigMeta::new(
            "Http",
            &[FieldMeta::new("port", "port").with_env_raw("PORT")],
        ));
    }

    struct App;
    impl ConfigSection for App {
        const META: &'static ConfigMeta = ConfigMeta::checked(&ConfigMeta::new(
            "App",
            &[
                FieldMeta::new("name", "name"),
                FieldMeta::new("common", "common")
                    .with_flatten(true)
                    .with_nested(
                        NestedMeta::new(NestedShape::Single, <Common as ConfigSection>::meta)
                            .with_section_ref(<Common as ConfigSection>::META),
                    ),
                FieldMeta::new("http", "http").with_nested(
                    NestedMeta::new(NestedShape::Single, <Http as ConfigSection>::meta)
                        .with_section_ref(<Http as ConfigSection>::META),
                ),
            ],
        ));
    }

    #[test]
    fn a_consistent_tree_passes() {
        assert_eq!(App::META.fields.len(), 3);
        assert_eq!(
            super::visible(App::META, 1).unwrap().field.name,
            "log_level"
        );
        assert!(super::visible(App::META, 3).is_none());
        assert_eq!(
            super::nth_raw(App::META, 1, 0).0.unwrap().field.name,
            "port"
        );
    }

    #[test]
    fn env_names_fold() {
        assert!(super::env_eq("max_connections", "MaxConnections"));
        assert!(super::env_eq("max-connections", "MAX_CONNECTIONS"));
        assert!(super::env_eq("maxConnections", "MAXCONNECTIONS"));
        assert!(!super::env_eq("max_connections", "maxconnection"));
    }

    fn panics(meta: &'static ConfigMeta) -> alloc::string::String {
        let result = std::panic::catch_unwind(|| ConfigMeta::checked(meta));
        let payload = result.expect_err("a clash");
        payload
            .downcast_ref::<alloc::string::String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).into()))
            .unwrap_or_default()
    }

    extern crate alloc;
    extern crate std;

    #[test]
    fn clashes_name_both_fields() {
        static FLAT_CLASH: ConfigMeta = ConfigMeta::new(
            "Root",
            &[
                FieldMeta::new("log_level", "log_level"),
                FieldMeta::new("common", "common")
                    .with_flatten(true)
                    .with_nested(
                        NestedMeta::new(NestedShape::Single, <Common as ConfigSection>::meta)
                            .with_section_ref(<Common as ConfigSection>::META),
                    ),
            ],
        );
        let message = panics(&FLAT_CLASH);
        assert!(
            message
                .contains("`Root.log_level` and `Common.log_level` both use the key `log_level`"),
            "{message}"
        );

        static RAW_CLASH: ConfigMeta = ConfigMeta::new(
            "Root",
            &[
                FieldMeta::new("listen", "listen").with_env_raw("port"),
                FieldMeta::new("http", "http").with_nested(
                    NestedMeta::new(NestedShape::Single, <Http as ConfigSection>::meta)
                        .with_section_ref(<Http as ConfigSection>::META),
                ),
            ],
        );
        let message = panics(&RAW_CLASH);
        assert!(message.contains("env_raw variable `port`"), "{message}");

        static ENV_CLASH: ConfigMeta = ConfigMeta::new(
            "Root",
            &[
                FieldMeta::new("max_conn", "max_conn"),
                FieldMeta::new("other", "other").with_env("MaxConn"),
            ],
        );
        assert!(panics(&ENV_CLASH).contains("env segment"));

        static SERDE_CLASH: ConfigMeta = ConfigMeta::new(
            "Root",
            &[
                FieldMeta::new("a", "same").with_doc_key("x"),
                FieldMeta::new("b", "same").with_doc_key("y"),
            ],
        );
        assert!(panics(&SERDE_CLASH).contains("serde key `same`"));

        static ALIAS_CLASH: ConfigMeta =
            ConfigMeta::new("Root", &[FieldMeta::new("a", "a").with_aliases(&["A"])]);
        assert!(panics(&ALIAS_CLASH).contains("both use the key `a`"));
    }
}
