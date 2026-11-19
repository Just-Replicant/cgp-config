//! clap helpers (feature `clap`): the bootstrap flags as `clap::Arg`s and reading them
//! back into a [`Bootstrap`].
//!
//! | Flag | Meaning |
//! |---|---|
//! | `--config <PATH>` | the main config file (must exist) |
//! | `--profile <NAME>` | the active profile |
//! | `--set <KEY=VALUE>` | an override (repeatable), the `CliArgs` layer |
//!
//! ```
//! use cgp_config_providers::bootstrap::Bootstrap;
//! use cgp_config_providers::cli;
//!
//! let command = cli::augment(clap::Command::new("app"));
//! let matches = command
//!     .try_get_matches_from(["app", "--profile", "prod", "--set", "http.port=9090"])
//!     .unwrap();
//! let done = Bootstrap::new("app").from_matches(&matches)?.finish_with(|_| false);
//! assert_eq!(done.active_profile, "prod");
//! # Ok::<(), cgp_config_core::ConfigError>(())
//! ```

use clap::{Arg, ArgAction, ArgMatches, Command};

use cgp_config_core::{ConfigError, Origin};

use crate::bootstrap::Bootstrap;
use crate::sources::CliOverrides;

/// The argument id of `--config`.
pub const CONFIG: &str = "config";
/// The argument id of `--profile`.
pub const PROFILE: &str = "profile";
/// The argument id of `--set`.
pub const SET: &str = "set";

/// The three bootstrap arguments.
#[must_use]
pub fn args() -> [Arg; 3] {
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
    /// Applies `--config`, `--profile`, and `--set` from `matches`; arguments that the
    /// command does not define are ignored.
    ///
    /// # Errors
    ///
    /// As [`CliOverrides::from_matches`].
    pub fn from_matches(mut self, matches: &ArgMatches) -> Result<Self, ConfigError> {
        let get = |id: &str| matches.try_get_one::<String>(id).ok().flatten().cloned();
        if let Some(path) = get(CONFIG) {
            self = self.config_path(path);
        }
        if let Some(profile) = get(PROFILE) {
            self = self.profile(profile);
        }
        Ok(self.cli_overrides(CliOverrides::from_matches(matches)?))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::augment;
    use crate::bootstrap::Bootstrap;
    use crate::sources::CliOverrides;

    #[test]
    fn flags_reach_the_bootstrap() {
        let command = augment(clap::Command::new("app"));
        let matches = command
            .clone()
            .try_get_matches_from([
                "app", "--config", "/x.toml", "--set", "a=1", "--set", "b.c=2",
            ])
            .unwrap();
        let done = Bootstrap::new("app")
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
