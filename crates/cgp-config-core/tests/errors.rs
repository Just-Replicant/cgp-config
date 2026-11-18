//! Every error and warning: stable code, path, and rendered message.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use cgp_config_core::{
    ConfigError, KeyPath, LoadReport, LoadWarning, Meta, Origin, RedactedRef, Span,
    ValidationErrors, ValidationIssue,
};

fn path(text: &str) -> KeyPath {
    KeyPath::parse(text).unwrap()
}

fn file_meta() -> Meta {
    Meta::new(Origin::file("app.toml")).with_span(Span::new(10, 14, 3, 7))
}

fn all_errors() -> Vec<ConfigError> {
    let mut overridden = Meta::new(Origin::env("APP__PORT"));
    overridden.shadow(&file_meta(), 16);
    let mut validation = ValidationErrors::new();
    validation.push(ValidationIssue::new("http.port", "port is 0"));
    validation.push(ValidationIssue::new("db.url", "must not be empty").with_code("app::empty"));
    vec![
        ConfigError::io(Origin::file("app.toml"), "permission denied"),
        ConfigError::parse(
            Origin::file("app.toml"),
            Some(Span::new(0, 1, 2, 5)),
            "expected `=`",
        ),
        ConfigError::parse(Origin::env_document("APP_CONFIG"), None, "trailing comma"),
        ConfigError::type_error(
            "http.port",
            "expected u16, found string \"x\"",
            Some(overridden),
        ),
        ConfigError::type_error(
            "db.password",
            "expected u32, found string \"hunter2\"",
            Some(Meta::new(Origin::env("PW")).with_secret(true)),
        ),
        ConfigError::type_error(KeyPath::root(), "expected a map, found integer 1", None),
        ConfigError::missing("db.url", ""),
        ConfigError::missing("db.url", "string"),
        ConfigError::unknown("http.prot", Some("port".into()), file_meta()),
        ConfigError::unknown("x", None, Meta::default()),
        ConfigError::duplicate_key("servers[1].name", file_meta()),
        ConfigError::duplicate_key("k", Meta::default()),
        ConfigError::MergeConflict {
            path: path("db"),
            base: Origin::file("a.toml"),
            overlay: Origin::env("APP__DB"),
            base_kind: "map",
            overlay_kind: "string",
        },
        ConfigError::Validation(validation),
        ConfigError::Validation(ValidationErrors::new()),
        ConfigError::Interpolate {
            path: path("url"),
            message: "undefined ${HOST}".into(),
        },
        ConfigError::InterpolateCycle { path: path("a") },
        ConfigError::AmbiguousEnv {
            key: "APP__A_B".into(),
            candidates: vec![path("a_b"), path("a.b")],
        },
        ConfigError::AmbiguousEnv {
            key: "APP__X".into(),
            candidates: vec![],
        },
        ConfigError::EnvEncoding {
            key: "APP__X".into(),
        },
        ConfigError::Encoding {
            origin: Origin::file("utf16.toml"),
        },
        ConfigError::SecretDenied {
            reference: RedactedRef::with_name("file", "db_password"),
        },
        ConfigError::SecretsDenied,
        ConfigError::Migration {
            from: 1,
            to: 2,
            message: "renamed key clashes".into(),
        },
        ConfigError::limit_exceeded("max_depth", 64),
        ConfigError::ReloadRejected {
            paths: vec![path("http.port")],
        },
        ConfigError::ReloadRejected { paths: vec![] },
        ConfigError::MissingProfile {
            profile: "prod".into(),
        },
    ]
}

#[test]
fn codes_and_messages_are_stable() {
    let rendered: Vec<String> = all_errors()
        .iter()
        .map(|e| format!("{} | {e}", e.code()))
        .collect();
    let expected = [
        "config::io | app.toml: permission denied",
        "config::parse | app.toml:2:5: syntax error: expected `=`",
        "config::parse | env document APP_CONFIG: syntax error: trailing comma",
        "config::type | http.port: expected u16, found string \"x\" at env APP__PORT (overrides app.toml:3:7)",
        "config::type | db.password: invalid secret value at env PW",
        "config::type | expected a map, found integer 1",
        "config::missing | missing db.url",
        "config::missing | missing db.url (expected string)",
        "config::unknown | unknown key http.prot at app.toml:3:7; did you mean \"port\"?",
        "config::unknown | unknown key x",
        "config::duplicate_key | duplicate key servers[1].name at app.toml:3:7",
        "config::duplicate_key | duplicate key k",
        "config::merge_conflict | cannot merge db: map from a.toml vs string from env APP__DB",
        "config::validation | validation failed (2 errors): http.port: port is 0; db.url: must not be empty",
        "config::validation | validation failed (0 errors)",
        "config::interpolate | interpolation at url: undefined ${HOST}",
        "config::interpolate_cycle | interpolation cycle at a",
        "config::ambiguous_env | ambiguous env key APP__A_B (matches a_b, a.b)",
        "config::ambiguous_env | ambiguous env key APP__X",
        "config::env_encoding | env value for APP__X is not valid UTF-8",
        "config::encoding | utf16.toml: not valid UTF-8",
        "config::secret_denied | secret reference rejected: file:db_password",
        "config::secrets_denied | secrets are disabled",
        "config::migration | migration v1\u{2192}v2: renamed key clashes",
        "config::limit_exceeded | limit exceeded: max_depth (64)",
        "config::reload_rejected | reload rejected; restart required (http.port)",
        "config::reload_rejected | reload rejected; restart required",
        "config::missing_profile | profile \"prod\" is not defined",
    ];
    for (got, want) in rendered.iter().zip(expected) {
        assert_eq!(got, want);
    }
    assert_eq!(rendered.len(), expected.len());
}

#[test]
fn paths_and_meta_are_exposed() {
    let errors = all_errors();
    let with_path: Vec<String> = errors
        .iter()
        .filter_map(|e| e.path().map(ToString::to_string))
        .collect();
    assert_eq!(
        with_path,
        [
            "http.port",
            "db.password",
            "(root)",
            "db.url",
            "db.url",
            "http.prot",
            "x",
            "servers[1].name",
            "k",
            "db",
            "url",
            "a"
        ]
    );
    assert_eq!(errors[3].meta().unwrap().origin, Origin::env("APP__PORT"));
    assert!(errors[0].meta().is_none());
    assert!(errors[8].meta().is_some());
}

#[test]
fn path_prefixes_apply_to_every_pathed_error() {
    let prefix = path("app.server");
    let prefixed: Vec<String> = all_errors()
        .into_iter()
        .map(|e| e.with_path_prefix(&prefix))
        .filter_map(|e| e.path().map(ToString::to_string))
        .collect();
    assert!(
        prefixed.iter().all(|p| p.starts_with("app.server")),
        "{prefixed:?}"
    );
    let validation = all_errors().swap_remove(13).with_path_prefix(&prefix);
    assert!(
        validation
            .to_string()
            .contains("app.server.http.port: port is 0")
    );
}

#[test]
fn secret_messages_never_leak() {
    let secret = &all_errors()[4];
    assert!(!secret.to_string().contains("hunter2"));
    let digest = RedactedRef::new("vault:kv/app?token=hunter2");
    assert_eq!(digest.scheme(), "vault");
    assert_eq!(digest, RedactedRef::new("vault:kv/app?token=hunter2"));
    assert_ne!(digest, RedactedRef::new("vault:kv/app?token=other"));
    assert_eq!(RedactedRef::new("no-scheme").scheme(), "unknown");
}

#[test]
fn validation_errors_collect_and_convert() {
    let mut errors = ValidationErrors::new();
    assert!(errors.is_empty());
    assert!(errors.clone().into_result().is_ok());
    errors.push(ValidationIssue::new("a", "bad").with_meta(Meta::new(Origin::Defaults)));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.iter().next().unwrap().code, "config::validation");
    let error = errors.into_result().unwrap_err();
    assert_eq!(error.to_string(), "validation failed (1 error): a: bad");
}

#[test]
fn errors_work_with_anyhow_and_std_error() {
    fn load() -> anyhow::Result<()> {
        Err(ConfigError::missing("db.url", ""))?;
        Ok(())
    }
    let error = load().unwrap_err().context("loading app config");
    assert_eq!(format!("{error:#}"), "loading app config: missing db.url");
    assert_eq!(
        error.downcast_ref::<ConfigError>().unwrap().code(),
        "config::missing"
    );
    let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(ConfigError::SecretsDenied);
    assert_eq!(boxed.to_string(), "secrets are disabled");
}

#[test]
fn warnings_have_codes_and_messages() {
    let warnings = vec![
        LoadWarning::AssumedVersion {
            origin: Origin::file("a.toml"),
        },
        LoadWarning::DocumentVersion {
            origin: Origin::file("a.toml"),
            version: 3,
        },
        LoadWarning::UnknownKey {
            path: path("http.prot"),
            suggestion: Some("port".into()),
            meta: Some(file_meta()),
        },
        LoadWarning::UnknownKey {
            path: path("x"),
            suggestion: None,
            meta: None,
        },
        LoadWarning::WorldWritable {
            origin: Origin::file("/etc/app.toml"),
        },
        LoadWarning::SecretWorldReadable {
            origin: Origin::file("/run/secrets/pw"),
        },
        LoadWarning::MissingProfile {
            profile: "prod".into(),
        },
        LoadWarning::IgnoredInclude {
            origin: Origin::file("a.toml"),
        },
        LoadWarning::DeprecatedKey {
            path: path("http.addr"),
            alias: "listen".into(),
            note: "renamed in 0.3".into(),
        },
    ];
    let rendered: Vec<String> = warnings
        .iter()
        .map(|w| format!("{} | {w}", w.code()))
        .collect();
    for line in &rendered {
        println!("{line}");
    }
    assert!(rendered.iter().all(|line| line.starts_with("config::")));
    assert!(
        rendered[2].contains("http.prot")
            && rendered[2].contains("port")
            && rendered[2].contains("app.toml")
    );

    let report = LoadReport::new();
    for warning in warnings.clone() {
        report.push(warning);
    }
    assert_eq!(report.len(), warnings.len());
    assert_eq!(report.warnings(), warnings);
    let cloned = report.clone();
    assert_eq!(cloned, report);
    assert_eq!(report.take().len(), warnings.len());
    assert!(report.is_empty());
    cloned.clear();
    assert!(cloned.is_empty());
}

#[test]
fn validation_collection_conversions() {
    let one = ConfigError::validation("a", "bad");
    assert_eq!(one.code(), "config::validation");
    let issues: ValidationErrors = [
        ValidationIssue::new("a", "x"),
        ValidationIssue::new("b", "y"),
    ]
    .into_iter()
    .collect();
    let mut all = ValidationErrors::from(ValidationIssue::new("c", "z"));
    all.extend(issues.iter().cloned());
    let paths: Vec<String> = (&all)
        .into_iter()
        .map(|issue| issue.path.to_string())
        .collect();
    assert_eq!(paths, ["c", "a", "b"]);
    assert_eq!(
        ConfigError::from(all).to_string(),
        "validation failed (3 errors): c: z; a: x; b: y"
    );
}

#[test]
fn type_errors_without_a_known_origin_still_show_overrides() {
    let mut meta = Meta::default();
    meta.shadow(&Meta::new(Origin::file("base.toml")), 4);
    let error = ConfigError::type_error("k", "expected u8, found string \"x\"", Some(meta));
    assert_eq!(
        error.to_string(),
        "k: expected u8, found string \"x\" (overrides base.toml)"
    );
    let digest = RedactedRef::new("vault:kv/app");
    let shown = ConfigError::SecretDenied { reference: digest }.to_string();
    assert!(
        shown.starts_with("secret reference rejected: vault:<redacted #"),
        "{shown}"
    );
    assert!(!shown.contains("kv/app"));
}
