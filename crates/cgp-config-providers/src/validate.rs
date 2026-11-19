//! Adapters from `garde` and `validator` to `ConfigValidator` (features `validate-garde`,
//! `validate-validator`). Users derive the library's trait on their config types; there
//! is no attribute proxy. Every reported problem becomes one [`ValidationIssue`](cgp_config_core::ValidationIssue), so all
//! of them reach the user in one `ConfigError::Validation`.
//!
//! Paths are those the library reports: garde follows serde-independent field names
//! (`http.port`, `servers[0].host`), as does validator. A `#[serde(rename)]`d field keeps
//! its Rust name in these paths.

#[cfg(any(feature = "validate-garde", feature = "validate-validator"))]
use cgp_config_core::KeyPath;

/// A library path as a [`KeyPath`], or a single key holding the text when it does not
/// parse.
#[cfg(any(feature = "validate-garde", feature = "validate-validator"))]
fn key_path(text: &str) -> KeyPath {
    if text.is_empty() {
        return KeyPath::root();
    }
    KeyPath::parse(text).unwrap_or_else(|_| {
        KeyPath::from_segments([cgp_config_core::PathSegment::Key(text.into())])
    })
}

#[cfg(feature = "validate-garde")]
pub use self::garde_adapter::{ValidateWithGarde, garde_issues};

#[cfg(feature = "validate-garde")]
mod garde_adapter {
    use cgp_config_core::{
        ConfigError, ConfigValidator, ConfigValidatorComponent, ValidationErrors, ValidationIssue,
    };
    use cgp_fork::prelude::*;

    use super::key_path;

    /// Validates with `garde::Validate` (its `Context` built by `Default`).
    pub struct ValidateWithGarde;

    /// The issues of a garde report.
    #[must_use]
    pub fn garde_issues(report: &garde::Report) -> ValidationErrors {
        report
            .iter()
            .map(|(path, error)| {
                ValidationIssue::new(key_path(&path.to_string()), error.message())
                    .with_code("garde")
            })
            .collect()
    }

    #[cgp_impl(ValidateWithGarde)]
    impl<Config> ConfigValidator<Config>
    where
        Self: CanRaiseError<ConfigError>,
        Config: garde::Validate,
        Config::Context: Default,
    {
        fn validate_config(&self, config: &Config) -> Result<(), Self::Error> {
            match config.validate() {
                Ok(()) => Ok(()),
                Err(report) => garde_issues(&report)
                    .into_result()
                    .map_err(Self::raise_error),
            }
        }
    }
}

#[cfg(feature = "validate-validator")]
pub use self::validator_adapter::{ValidateWithValidator, validator_issues};

#[cfg(feature = "validate-validator")]
mod validator_adapter {
    use cgp_config_core::{
        ConfigError, ConfigValidator, ConfigValidatorComponent, KeyPath, PathSegment,
        ValidationErrors, ValidationIssue,
    };
    use cgp_fork::prelude::*;
    use validator::ValidationErrorsKind;

    /// Validates with `validator::Validate`.
    pub struct ValidateWithValidator;

    /// The issues of validator errors, sorted by path.
    #[must_use]
    pub fn validator_issues(errors: &validator::ValidationErrors) -> ValidationErrors {
        let mut out = Vec::new();
        collect(errors, &KeyPath::root(), &mut out);
        out.sort_by_key(|issue: &ValidationIssue| issue.path.to_string());
        out.into_iter().collect()
    }

    fn collect(errors: &validator::ValidationErrors, at: &KeyPath, out: &mut Vec<ValidationIssue>) {
        for (field, kind) in errors.errors() {
            // `__all__` holds struct-level (schema) errors.
            let path = if field == "__all__" {
                at.clone()
            } else {
                at.join(field.as_ref())
            };
            match kind {
                ValidationErrorsKind::Field(list) => {
                    for error in list {
                        let message = error
                            .message
                            .as_deref()
                            .map_or_else(|| format!("failed `{}`", error.code), str::to_owned);
                        out.push(
                            ValidationIssue::new(path.clone(), message).with_code("validator"),
                        );
                    }
                }
                ValidationErrorsKind::Struct(inner) => collect(inner, &path, out),
                ValidationErrorsKind::List(items) => {
                    for (index, inner) in items {
                        let mut element = path.clone();
                        element.push(PathSegment::Index(*index));
                        collect(inner, &element, out);
                    }
                }
            }
        }
    }

    #[cgp_impl(ValidateWithValidator)]
    impl<Config> ConfigValidator<Config>
    where
        Self: CanRaiseError<ConfigError>,
        Config: validator::Validate,
    {
        fn validate_config(&self, config: &Config) -> Result<(), Self::Error> {
            match config.validate() {
                Ok(()) => Ok(()),
                Err(errors) => validator_issues(&errors)
                    .into_result()
                    .map_err(Self::raise_error),
            }
        }
    }
}

#[cfg(all(test, any(feature = "validate-garde", feature = "validate-validator")))]
mod tests {
    use super::key_path;

    #[test]
    fn paths() {
        assert_eq!(key_path(""), cgp_config_core::KeyPath::root());
        assert_eq!(key_path("servers[0].host").to_string(), "servers[0].host");
    }
}
