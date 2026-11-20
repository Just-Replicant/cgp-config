use quote::quote;

use crate::derive_config_section;

fn expand(input: proc_macro2::TokenStream) -> String {
    derive_config_section(input).to_string()
}

#[test]
fn emits_meta_and_validation() {
    let out = expand(quote! {
        /// The app.
        #[serde(rename_all = "camelCase")]
        #[config(section = "app", version = 2, env_prefix = "APP", deny_unknown)]
        struct App<T> {
            /// Port to bind.
            #[config(default = 8080, env_raw = "PORT")]
            listen_port: u16,
            #[config(default = "30s", parse = "duration")]
            timeout: Duration,
            #[config(default = -1.5)]
            ratio: f64,
            #[config(default = ["a", "b"], merge = "append")]
            tags: Vec<String>,
            #[config(secret, alias = "pw", deprecated = "use `password`")]
            password: Option<String>,
            #[serde(rename = "db")]
            #[config(nested)]
            database: Db,
            #[config(nested)]
            servers: Vec<Server>,
            #[config(nested)]
            pools: BTreeMap<String, Pool>,
            #[config(flatten)]
            common: Common,
            #[serde(skip)]
            cache: T,
            name: String,
        }
    });
    for expected in [
        "FieldMeta :: new (\"listen_port\" , \"listenPort\")",
        "with_doc (\"Port to bind.\")",
        "ConstDefault :: UInt (8080)",
        "with_env_raw (\"PORT\")",
        "ParseKind :: Duration",
        "ConstDefault :: Float (- 1.5)",
        "ConstDefault :: Array (& [__cgp :: ConstDefault :: Str (\"a\") , __cgp :: ConstDefault :: Str (\"b\")])",
        "FieldMerge :: Append",
        "with_aliases (& [\"pw\"])",
        "with_secret (true)",
        "with_optional (true)",
        "FieldMeta :: new (\"database\" , \"db\")",
        "NestedShape :: Seq",
        "NestedShape :: Map",
        "with_flatten (true)",
        "with_section (\"app\")",
        "with_version (2u32)",
        "with_env_prefix (\"APP\")",
        "with_deny_unknown (true)",
        "with_doc (\"The app.\")",
        "nested :: single (__context , \"db\"",
        "nested :: seq (__context , \"servers\"",
        "nested :: map (__context , \"pools\"",
        "CanValidateConfig < Common >",
    ] {
        assert!(out.contains(expected), "missing `{expected}` in\n{out}");
    }
    assert!(!out.contains("\"cache\""), "skipped fields are absent");
    assert!(
        out.contains("ConfigMeta :: checked (&"),
        "META runs the compile-time key check:\n{out}"
    );
    assert!(
        out.contains("with_section_ref (< Db as __cgp :: ConfigSection > :: META)"),
        "single sections carry a direct reference:\n{out}"
    );
    assert!(out.contains("with_section_ref (< Common as __cgp :: ConfigSection > :: META)"));
    assert!(
        !out.contains("with_section_ref (< Server"),
        "lists stay behind a fn pointer"
    );
    assert!(
        !out.contains("with_section_ref (< Pool"),
        "maps stay behind a fn pointer"
    );
    assert!(
        !out.contains("const _ : & 'static __cgp :: ConfigMeta"),
        "a generic section is checked on first use, not at its declaration"
    );
    let name_meta = out.split("FieldMeta :: new (\"name\"").nth(1).unwrap();
    assert!(name_meta.contains("with_required (true)"));
    let port_meta = out
        .split("FieldMeta :: new (\"listen_port\"")
        .nth(1)
        .unwrap();
    let port_meta = port_meta.split("FieldMeta :: new").next().unwrap();
    assert!(!port_meta.contains("with_required"));
}

fn errors(input: proc_macro2::TokenStream) -> Vec<String> {
    let out = expand(input);
    out.split("compile_error !")
        .skip(1)
        .map(|part| part.split('"').nth(1).unwrap_or_default().to_owned())
        .collect()
}

#[test]
fn misuse_is_reported_together() {
    let messages = errors(quote! {
        #[config(migrate(from = 1))]
        #[config(bogus)]
        struct Bad {
            #[config(default)]
            a: u16,
            #[config(parse = "duration", default = 30)]
            b: Duration,
            #[config(nested, default = 1)]
            c: Inner,
            #[config(env = "A__B")]
            #[config(reload = "sometimes")]
            d: u8,
            #[config(skip)]
            e: u8,
            #[serde(rename = "$version")]
            f: u8,
            #[config(flatten)]
            #[serde(flatten)]
            g: Inner,
            #[config(alias = "h")]
            i: u8,
            h: u8,
        }
    });
    let expected = [
        "available from cgp-config 0.2",
        "unknown `config` attribute `bogus`",
        "`default` needs a value",
        "a duration default is a string with a unit",
        "`default` cannot be used on a nested section",
        "must not contain the separator",
        "unknown reload policy",
        "`config(skip)` needs `#[serde(skip)]`",
        "keys starting with `$` are reserved",
        "use `#[config(flatten)]` alone",
        "key `h` is also used by `i`",
    ];
    for expected in expected {
        assert!(
            messages.iter().any(|m| m.contains(expected)),
            "missing `{expected}` in {messages:#?}"
        );
    }
}

#[test]
fn rejects_non_structs() {
    assert!(
        errors(quote!(
            enum E {
                A,
            }
        ))
        .iter()
        .any(|m| m.contains("an enum is a value"))
    );
    assert!(
        errors(quote!(
            struct T(u8);
        ))
        .iter()
        .any(|m| m.contains("named fields"))
    );
    assert!(
        errors(quote!(union U { a: u8 }))
            .iter()
            .any(|m| m.contains("union"))
    );
    assert!(!expand(quote!(not a struct)).is_empty());
    // A unit struct is an empty section.
    assert!(
        expand(quote!(
            struct Empty;
        ))
        .contains("ConfigMeta :: new (\"Empty\" , & [])")
    );
}

#[test]
fn rename_rules_must_agree() {
    let messages = errors(quote! {
        #[serde(rename_all = "camelCase")]
        #[config(rename_all = "kebab-case")]
        struct S { a_b: u8 }
    });
    assert!(
        messages.iter().any(|m| m.contains("disagrees")),
        "{messages:?}"
    );
    let out = expand(quote! {
        #[config(rename_all = "kebab-case")]
        struct S { a_b: u8 }
    });
    assert!(out.contains("\"a-b\""), "{out}");
}
