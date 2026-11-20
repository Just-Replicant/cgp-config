//! clap helpers (feature `clap`): the bootstrap flags as `clap::Arg`s, reading them back
//! into a [`Bootstrap`], and the `config` subcommand ([`config_command`]).
//!
//! | Flag | Meaning |
//! |---|---|
//! | `--config <PATH>` | the main config file (must exist) |
//! | `--profile <NAME>` | the active profile |
//! | `--env-prefix <PREFIX>` | the env prefix (default: the app name, uppercased) |
//! | `--set <KEY=VALUE>` | an override (repeatable), the `CliArgs` layer |
//!
//! `--set` is accepted only when the bootstrap names a layer list with `CliArgs`
//! ([`Bootstrap::layers`]); otherwise [`Bootstrap::from_matches`] fails rather than
//! collecting overrides that no layer reads.
//!
//! The env separator is always `__`; the design plan's single-underscore
//! `LossyUnderscore` mode is not available in this release (see [`crate::env`]), so
//! there is no flag for it.
//!
//! ```
//! use cgp_config_providers::bootstrap::Bootstrap;
//! use cgp_config_providers::cli;
//! use cgp_config_providers::layers::TwelveFactorCliLayers;
//!
//! let command = cli::augment(clap::Command::new("app"));
//! let matches = command
//!     .try_get_matches_from(["app", "--profile", "prod", "--env-prefix", "SVC", "--set", "http.port=9090"])
//!     .unwrap();
//! let done = Bootstrap::new("app")
//!     .layers::<TwelveFactorCliLayers>()
//!     .from_matches(&matches)?
//!     .finish_with(|_| false);
//! assert_eq!(done.active_profile, "prod");
//! assert_eq!(done.env_prefix, "SVC");
//! # Ok::<(), cgp_config_core::ConfigError>(())
//! ```

use core::fmt::Write as _;

use cgp_config_core::{ConfigError, ConfigMeta, ConstDefault, NestedShape, Origin};
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::bootstrap::Bootstrap;
use crate::env::SEPARATOR;
use crate::sources::CliOverrides;

/// The argument id of `--config`.
pub const CONFIG: &str = "config";
/// The argument id of `--profile`.
pub const PROFILE: &str = "profile";
/// The argument id of `--env-prefix`.
pub const ENV_PREFIX: &str = "env-prefix";
/// The argument id of `--set`.
pub const SET: &str = "set";

/// The four bootstrap arguments.
#[must_use]
pub fn args() -> [Arg; 4] {
    [
        Arg::new(CONFIG)
            .long("config")
            .value_name("PATH")
            .help("Config file (default: search the standard locations)")
            .action(ArgAction::Set),
        Arg::new(PROFILE)
            .long("profile")
            .value_name("NAME")
            .help("Active profile")
            .action(ArgAction::Set),
        Arg::new(ENV_PREFIX)
            .long("env-prefix")
            .value_name("PREFIX")
            .help("Prefix of the config env variables (default: the app name, uppercased)")
            .action(ArgAction::Set),
        Arg::new(SET)
            .long("set")
            .value_name("KEY=VALUE")
            .help("Override one config value (repeatable)")
            .action(ArgAction::Append),
    ]
}

/// `command` with [`args`] added.
#[must_use]
pub fn augment(command: Command) -> Command {
    command.args(args())
}

/// The id of the `PATH` argument of `explain`.
pub const EXPLAIN_PATH: &str = "path";

/// A `config` subcommand for the app to attach under any name (design plan §6.12):
///
/// | Subcommand | Meaning |
/// |---|---|
/// | `check` | load the config and report errors and warnings |
/// | `explain <PATH>` | show the value at `PATH` and where it came from |
/// | `template` | print a TOML template of every field ([`render_template`]) |
/// | `schema` | print every field with its type, default, and env name ([`render_schema`]) |
///
/// The command only parses; [`ConfigCommand::from_matches`] reads the choice back, and
/// the app runs it with its own loader (`check` is a `load_config` plus the load
/// report). A JSON Schema export (`SchemarsSchema`, feature `schema`) and provenance
/// dumps for `explain` are later phases; `schema` prints the field table.
///
/// ```
/// use cgp_config_providers::cli::{ConfigCommand, config_command};
///
/// let app = clap::Command::new("app").subcommand(config_command("config"));
/// let matches = app.try_get_matches_from(["app", "config", "explain", "http.port"]).unwrap();
/// let (_, sub) = matches.subcommand().unwrap();
/// assert_eq!(
///     ConfigCommand::from_matches(sub),
///     Some(ConfigCommand::Explain("http.port".into()))
/// );
/// ```
#[must_use]
pub fn config_command(name: impl Into<clap::builder::Str>) -> Command {
    Command::new(name)
        .about("Inspect the configuration")
        .subcommand_required(true)
        .subcommand(Command::new("check").about("Load the configuration and report problems"))
        .subcommand(
            Command::new("explain")
                .about("Show a value and where it came from")
                .arg(
                    Arg::new(EXPLAIN_PATH)
                        .value_name("PATH")
                        .help("Document path, e.g. http.port or servers[0].host")
                        .required(true),
                ),
        )
        .subcommand(Command::new("template").about("Print a TOML template of every field"))
        .subcommand(
            Command::new("schema").about("Print every field with its type, default, and env name"),
        )
}

/// A parsed [`config_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigCommand {
    /// `check`.
    Check,
    /// `explain <PATH>`.
    Explain(String),
    /// `template`.
    Template,
    /// `schema`.
    Schema,
}

impl ConfigCommand {
    /// The subcommand chosen in `matches` (the matches of the `config` command itself).
    #[must_use]
    pub fn from_matches(matches: &ArgMatches) -> Option<Self> {
        match matches.subcommand()? {
            ("check", _) => Some(Self::Check),
            ("explain", sub) => sub
                .get_one::<String>(EXPLAIN_PATH)
                .map(|path| Self::Explain(path.clone())),
            ("template", _) => Some(Self::Template),
            ("schema", _) => Some(Self::Schema),
            _ => None,
        }
    }
}

/// A TOML template of `meta`: every field with its doc comment, defaults as values,
/// fields without a default commented out, secrets as a comment only, nested sections
/// as tables (flattened ones inline on the parent).
///
/// ```
/// use cgp_config_core::{ConfigMeta, ConstDefault, FieldMeta};
/// use cgp_config_providers::cli::render_template;
///
/// static META: ConfigMeta = ConfigMeta::new(
///     "App",
///     &[
///         FieldMeta::new("port", "port").with_default(ConstDefault::UInt(8080)).with_doc("Port"),
///         FieldMeta::new("token", "token").with_secret(true),
///     ],
/// );
/// let template = render_template(&META);
/// assert!(template.contains("# Port\nport = 8080\n"));
/// assert!(template.contains("# token: secret, set via a secret reference"));
/// ```
#[must_use]
pub fn render_template(meta: &'static ConfigMeta) -> String {
    let mut out = String::new();
    template_section(&mut out, meta, "", 0);
    out
}

fn template_section(out: &mut String, meta: &'static ConfigMeta, table: &str, depth: usize) {
    if depth >= cgp_config_core::FieldChain::CAPACITY {
        return;
    }
    let mut nested = Vec::new();
    template_fields(out, meta, &mut nested, depth);
    for (key, shape, section) in nested {
        let name = if table.is_empty() {
            key.to_owned()
        } else {
            format!("{table}.{key}")
        };
        out.push('\n');
        match shape {
            NestedShape::Seq => {
                let _ = writeln!(out, "# [[{name}]]");
                let mut inner = String::new();
                template_section(&mut inner, section, &name, depth + 1);
                for line in inner.lines() {
                    let _ = writeln!(out, "# {line}");
                }
            }
            NestedShape::Map => {
                let _ = writeln!(out, "# [{name}.<name>]");
                let mut inner = String::new();
                template_section(&mut inner, section, &format!("{name}.<name>"), depth + 1);
                for line in inner.lines() {
                    let _ = writeln!(out, "# {line}");
                }
            }
            _ => {
                let _ = writeln!(out, "[{name}]");
                template_section(out, section, &name, depth + 1);
            }
        }
    }
}

fn template_fields(
    out: &mut String,
    meta: &'static ConfigMeta,
    nested: &mut Vec<(&'static str, NestedShape, &'static ConfigMeta)>,
    depth: usize,
) {
    for field in meta.fields {
        if let Some(section_meta) = field.nested {
            if field.flatten && depth < cgp_config_core::FieldChain::CAPACITY {
                template_fields(out, section_meta.section(), nested, depth + 1);
            } else {
                nested.push((field.doc_key, section_meta.shape, section_meta.section()));
            }
            continue;
        }
        for line in field.doc.lines().filter(|line| !line.trim().is_empty()) {
            let _ = writeln!(out, "# {}", line.trim());
        }
        if field.secret {
            let _ = writeln!(
                out,
                "# {}: secret, set via a secret reference",
                field.doc_key
            );
            continue;
        }
        match field.default {
            Some(default) => {
                let _ = writeln!(out, "{} = {}", field.doc_key, toml_value(default));
            }
            None if field.required => {
                let _ = writeln!(out, "# {} = ...  (required, {})", field.doc_key, field.ty);
            }
            None => {
                let _ = writeln!(out, "# {} = ...  ({})", field.doc_key, field.ty);
            }
        }
    }
}

fn toml_value(default: ConstDefault) -> String {
    match default {
        ConstDefault::Bool(value) => value.to_string(),
        ConstDefault::Int(value) => value.to_string(),
        ConstDefault::UInt(value) => value.to_string(),
        ConstDefault::Float(value) => format!("{value:?}"),
        ConstDefault::Str(value) => format!("{value:?}"),
        ConstDefault::Array(values) => {
            let items: Vec<String> = values.iter().copied().map(toml_value).collect();
            format!("[{}]", items.join(", "))
        }
        #[allow(unreachable_patterns)]
        _ => "...".to_owned(),
    }
}

/// One line per leaf field of `meta`: document path, type, default, and the env
/// variable that sets it under `prefix` (`http.port  u16  default 8080  APP__HTTP__PORT`).
/// Secrets show no default.
///
/// ```
/// use cgp_config_core::{ConfigMeta, ConstDefault, FieldMeta};
/// use cgp_config_providers::cli::render_schema;
///
/// static META: ConfigMeta = ConfigMeta::new(
///     "App",
///     &[FieldMeta::new("port", "port").with_default(ConstDefault::UInt(8080)).with_type("u16")],
/// );
/// assert_eq!(render_schema(&META, "APP"), "port  u16  default 8080  APP__PORT\n");
/// ```
#[must_use]
pub fn render_schema(meta: &'static ConfigMeta, prefix: &str) -> String {
    let mut out = String::new();
    schema_section(&mut out, meta, "", prefix, 0);
    out
}

fn schema_section(
    out: &mut String,
    meta: &'static ConfigMeta,
    path: &str,
    env: &str,
    depth: usize,
) {
    if depth >= cgp_config_core::FieldChain::CAPACITY {
        return;
    }
    for field in meta.fields {
        let env_segment = field
            .env
            .unwrap_or(field.doc_key)
            .replace('-', "_")
            .to_ascii_uppercase();
        if let Some(nested) = field.nested {
            if field.flatten {
                schema_section(out, nested.section(), path, env, depth + 1);
                continue;
            }
            let (child_path, child_env) = match nested.shape {
                NestedShape::Single => (
                    join(path, field.doc_key),
                    format!("{env}{SEPARATOR}{env_segment}"),
                ),
                NestedShape::Seq => (format!("{}[]", join(path, field.doc_key)), String::new()),
                _ => (
                    format!("{}.<name>", join(path, field.doc_key)),
                    format!("{env}{SEPARATOR}{env_segment}{SEPARATOR}<NAME>"),
                ),
            };
            schema_section(out, nested.section(), &child_path, &child_env, depth + 1);
            continue;
        }
        let _ = write!(out, "{}  {}", join(path, field.doc_key), field.ty);
        match field.default.filter(|_| !field.secret) {
            Some(default) => {
                let _ = write!(out, "  default {}", toml_value(default));
            }
            None if field.required => out.push_str("  required"),
            None => {}
        }
        if field.secret {
            out.push_str("  secret");
        }
        if !env.is_empty() || depth == 0 {
            let _ = write!(out, "  {env}{SEPARATOR}{env_segment}");
        }
        if let Some(raw) = field.env_raw {
            let _ = write!(out, "  {raw}");
        }
        out.push('\n');
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

impl CliOverrides {
    /// The `--set` values of `matches` (none when the argument is not defined).
    ///
    /// # Errors
    ///
    /// `ConfigError::Parse` (origin `cli --set`) for a value without `=`.
    pub fn from_matches(matches: &ArgMatches) -> Result<Self, ConfigError> {
        let mut overrides = Self::new();
        let values = matches.try_get_many::<String>(SET).ok().flatten();
        for assignment in values.into_iter().flatten() {
            overrides
                .insert_assignment(assignment)
                .map_err(|message| ConfigError::parse(Origin::cli("--set"), None, message))?;
        }
        Ok(overrides)
    }
}

impl Bootstrap {
    /// Applies `--config`, `--profile`, `--env-prefix`, and `--set` from `matches`;
    /// arguments that the command does not define are ignored.
    ///
    /// # Errors
    ///
    /// As [`CliOverrides::from_matches`], and `ConfigError::Parse` for `--set` values
    /// when the layer list ([`Bootstrap::layers`]) has no `CliArgs`.
    pub fn from_matches(mut self, matches: &ArgMatches) -> Result<Self, ConfigError> {
        let get = |id: &str| matches.try_get_one::<String>(id).ok().flatten().cloned();
        if let Some(path) = get(CONFIG) {
            self = self.config_path(path);
        }
        if let Some(profile) = get(PROFILE) {
            self = self.profile(profile);
        }
        if let Some(prefix) = get(ENV_PREFIX) {
            self = self.env_prefix(prefix);
        }
        let overrides = CliOverrides::from_matches(matches)?;
        if !overrides.is_empty() && !self.accepts_cli_overrides() {
            return Err(ConfigError::parse(
                Origin::cli("--set"),
                None,
                "`--set` needs a loader with a `CliArgs` layer; name it with \
                 `Bootstrap::layers` (e.g. `TwelveFactorCliLayers`)",
            ));
        }
        Ok(self.cli_overrides(overrides))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use cgp_config_core::{ConfigMeta, ConstDefault, FieldMeta, NestedMeta, NestedShape};

    use super::{ConfigCommand, augment, config_command, render_schema, render_template};
    use crate::bootstrap::Bootstrap;
    use crate::layers::TwelveFactorCliLayers;
    use crate::sources::CliOverrides;

    #[test]
    fn env_prefix_flag() {
        let matches = augment(clap::Command::new("app"))
            .try_get_matches_from(["app", "--env-prefix", "SVC"])
            .unwrap();
        let done = Bootstrap::new("app")
            .with_env(cgp_config_core::EnvSnapshot::from_iter([(
                "SVC_PROFILE",
                "p",
            )]))
            .from_matches(&matches)
            .unwrap()
            .finish_with(|_| false);
        assert_eq!(done.env_prefix, "SVC");
        assert_eq!(done.active_profile, "p");
    }

    #[test]
    fn config_subcommands_parse() {
        let parse = |args: &[&str]| {
            let matches = config_command("config").try_get_matches_from(args).unwrap();
            ConfigCommand::from_matches(&matches)
        };
        assert_eq!(parse(&["config", "check"]), Some(ConfigCommand::Check));
        assert_eq!(
            parse(&["config", "template"]),
            Some(ConfigCommand::Template)
        );
        assert_eq!(parse(&["config", "schema"]), Some(ConfigCommand::Schema));
        assert_eq!(
            parse(&["config", "explain", "a.b"]),
            Some(ConfigCommand::Explain("a.b".into()))
        );
        assert!(
            config_command("config")
                .try_get_matches_from(["config"])
                .is_err()
        );
        assert!(
            config_command("config")
                .try_get_matches_from(["config", "explain"])
                .is_err()
        );
    }

    struct Http;
    impl cgp_config_core::ConfigSection for Http {
        const META: &'static ConfigMeta = &ConfigMeta::new(
            "Http",
            &[
                FieldMeta::new("port", "port")
                    .with_default(ConstDefault::UInt(8080))
                    .with_type("u16"),
                FieldMeta::new("host", "host")
                    .with_required(true)
                    .with_type("String"),
            ],
        );
    }

    fn app() -> &'static ConfigMeta {
        const META: &ConfigMeta = &ConfigMeta::new(
            "App",
            &[
                FieldMeta::new("name", "name")
                    .with_default(ConstDefault::Str("svc"))
                    .with_type("String"),
                FieldMeta::new("http", "http").with_nested(NestedMeta::new(
                    NestedShape::Single,
                    <Http as cgp_config_core::ConfigSection>::meta,
                )),
                FieldMeta::new("token", "token")
                    .with_secret(true)
                    .with_type("String"),
            ],
        );
        META
    }

    #[test]
    fn template_and_schema_render() {
        let template = render_template(app());
        assert!(template.contains("name = \"svc\"\n"), "{template}");
        assert!(
            template.contains("\n[http]\nport = 8080\n# host = ...  (required, String)\n"),
            "{template}"
        );
        assert!(template.contains("# token: secret"), "{template}");
        let schema = render_schema(app(), "APP");
        assert!(
            schema.contains("http.port  u16  default 8080  APP__HTTP__PORT\n"),
            "{schema}"
        );
        assert!(
            schema.contains("http.host  String  required  APP__HTTP__HOST\n"),
            "{schema}"
        );
        assert!(
            schema.contains("token  String  secret  APP__TOKEN\n"),
            "{schema}"
        );
    }

    #[test]
    fn flags_reach_the_bootstrap() {
        let command = augment(clap::Command::new("app"));
        let matches = command
            .clone()
            .try_get_matches_from([
                "app", "--config", "/x.toml", "--set", "a=1", "--set", "b.c=2",
            ])
            .unwrap();
        assert!(
            Bootstrap::new("app").from_matches(&matches).is_err(),
            "no CliArgs layer"
        );
        let done = Bootstrap::new("app")
            .layers::<TwelveFactorCliLayers>()
            .from_matches(&matches)
            .unwrap()
            .finish_with(|_| false);
        assert_eq!(done.config_location.file, Some(PathBuf::from("/x.toml")));
        assert_eq!(done.active_profile, "default");
        assert_eq!(done.cli_overrides.iter().count(), 2);

        let bad = command
            .try_get_matches_from(["app", "--set", "oops"])
            .unwrap();
        assert_eq!(
            CliOverrides::from_matches(&bad).unwrap_err().code(),
            "config::parse"
        );

        let other = clap::Command::new("other")
            .try_get_matches_from(["other"])
            .unwrap();
        assert!(CliOverrides::from_matches(&other).unwrap().is_empty());
    }
}
