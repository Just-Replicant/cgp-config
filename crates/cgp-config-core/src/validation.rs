//! Composable validators: [`ValidateFn`], [`AllOf`], [`Nested`], and the
//! [`AggregateError`] contract they share.
//!
//! Validation never stops at the first failure: every provider here runs to the end and
//! merges what it found, so one load reports every problem (design plan §6.7).
//!
//! ```text
//! ConfigValidatorComponent: UseDelegate<new AppValidators {
//!     AppConfig: AllOf<Product![Nested, ValidateFn<PortsDiffer>]>,
//!     HttpConfig: ValidateFn<HttpRules>,
//!     [TlsConfig, DbConfig]: NoValidation,
//! }>,
//! ```

use alloc::string::ToString;

use cgp_fork::prelude::*;

use crate::{
    CanValidateConfig, ConfigError, ConfigValidator, ConfigValidatorComponent, KeyPath,
    ValidationErrors, ValidationIssue,
};

/// An error that can absorb another error of the same load, and move under a parent
/// path. Implemented for [`ConfigError`]; a context with its own error type implements it
/// to use [`AllOf`] and [`Nested`].
pub trait AggregateError: Sized {
    /// Combines two failures into one that reports both.
    #[must_use]
    fn aggregate(self, other: Self) -> Self;

    /// Prefixes every path in the error with `prefix`.
    #[must_use]
    fn at_path(self, prefix: &KeyPath) -> Self;
}

impl AggregateError for ConfigError {
    /// Validation errors concatenate. Any other error becomes one [`ValidationIssue`] that
    /// keeps its code, path, and provenance, so nothing is lost.
    fn aggregate(self, other: Self) -> Self {
        let mut issues = into_issues(self);
        issues.extend(into_issues(other).items);
        Self::Validation(issues)
    }

    fn at_path(self, prefix: &KeyPath) -> Self {
        self.with_path_prefix(prefix)
    }
}

fn into_issues(error: ConfigError) -> ValidationErrors {
    match error {
        ConfigError::Validation(errors) => errors,
        other => {
            let mut issue = ValidationIssue::new(
                other.path().cloned().unwrap_or_else(KeyPath::root),
                other.to_string(),
            )
            .with_code(other.code());
            if let Some(meta) = other.meta() {
                issue = issue.with_meta(meta.clone());
            }
            ValidationErrors::from(issue)
        }
    }
}

/// Folds `next` into an optional accumulated error.
pub fn accumulate<E: AggregateError>(acc: Option<E>, next: E) -> Option<E> {
    Some(match acc {
        Some(acc) => acc.aggregate(next),
        None => next,
    })
}

/// A plain validation rule over one config type, for [`ValidateFn`]. Push every failure;
/// do not stop at the first.
///
/// ```
/// use cgp_config_core::{ValidationErrors, ValidationIssue, ValidationRule};
///
/// struct Http { port: u16, admin_port: u16 }
/// struct PortsDiffer;
///
/// impl ValidationRule<Http> for PortsDiffer {
///     fn check(http: &Http, errors: &mut ValidationErrors) {
///         if http.port == http.admin_port {
///             errors.push(ValidationIssue::new("admin_port", "must differ from port"));
///         }
///     }
/// }
/// ```
pub trait ValidationRule<Config> {
    /// Records every failure of `config` in `errors`.
    fn check(config: &Config, errors: &mut ValidationErrors);
}

/// Runs the [`ValidationRule`] `Rule`.
pub struct ValidateFn<Rule>(PhantomData<Rule>);

#[cgp_impl(ValidateFn<Rule>)]
impl<Rule, Config> ConfigValidator<Config>
where
    Self: CanRaiseError<ConfigError>,
    Rule: ValidationRule<Config>,
{
    fn validate_config(&self, config: &Config) -> Result<(), Self::Error> {
        let mut errors = ValidationErrors::new();
        Rule::check(config, &mut errors);
        errors.into_result().map_err(Self::raise_error)
    }
}

/// A `Product![..]` of validator providers, run in order by [`AllOf`].
pub trait ValidatorList<Context: HasErrorType, Config> {
    /// Runs every validator and folds their errors into `acc`.
    fn validate_all(
        context: &Context,
        config: &Config,
        acc: Option<Context::Error>,
    ) -> Option<Context::Error>;
}

impl<Context: HasErrorType, Config> ValidatorList<Context, Config> for Nil {
    fn validate_all(
        _context: &Context,
        _config: &Config,
        acc: Option<Context::Error>,
    ) -> Option<Context::Error> {
        acc
    }
}

impl<Context, Config, Head, Tail> ValidatorList<Context, Config> for Cons<Head, Tail>
where
    Context: HasErrorType,
    Context::Error: AggregateError,
    Head: ConfigValidator<Context, Config>,
    Tail: ValidatorList<Context, Config>,
{
    fn validate_all(
        context: &Context,
        config: &Config,
        acc: Option<Context::Error>,
    ) -> Option<Context::Error> {
        let acc = match Head::validate_config(context, config) {
            Ok(()) => acc,
            Err(error) => accumulate(acc, error),
        };
        Tail::validate_all(context, config, acc)
    }
}

/// Runs every validator of `Validators` (a `Product![..]` of providers) and reports all
/// their failures together.
pub struct AllOf<Validators>(PhantomData<Validators>);

#[cgp_impl(AllOf<Validators>)]
impl<Validators, Config> ConfigValidator<Config>
where
    Self: HasErrorType,
    Validators: ValidatorList<Self, Config>,
{
    fn validate_config(&self, config: &Config) -> Result<(), Self::Error> {
        match Validators::validate_all(self, config, None) {
            None => Ok(()),
            Some(error) => Err(error),
        }
    }
}

/// The nested sections of a config type, each validated through the context's own
/// [`CanValidateConfig`] wiring. `#[derive(ConfigSection)]` implements it from the fields
/// marked `#[config(nested)]` (and does nothing for a section without them).
pub trait ValidateSections<Context: HasErrorType> {
    /// Validates every nested section, prefixing paths with the field key and folding all
    /// failures together.
    ///
    /// # Errors
    ///
    /// Every failure of every nested section.
    fn validate_sections(&self, context: &Context) -> Result<(), Context::Error>;
}

/// Validates each `#[config(nested)]` field of a section with the context's validator for
/// that field's type (`T: ValidateSections<Self>`, emitted by the derive).
pub struct Nested;

#[cgp_impl(Nested)]
impl<Config> ConfigValidator<Config>
where
    Self: HasErrorType,
    Config: ValidateSections<Self>,
{
    fn validate_config(&self, config: &Config) -> Result<(), Self::Error> {
        config.validate_sections(self)
    }
}

/// Helpers the derive calls for each nested field shape. Public for generated code.
#[doc(hidden)]
pub mod nested {
    use super::{AggregateError, CanValidateConfig, KeyPath, accumulate};
    use crate::PathSegment;

    /// One nested section at `key`.
    pub fn single<Context, T>(
        context: &Context,
        key: &'static str,
        value: &T,
        acc: Option<Context::Error>,
    ) -> Option<Context::Error>
    where
        Context: CanValidateConfig<T>,
        Context::Error: AggregateError,
    {
        match context.validate_config(value) {
            Ok(()) => acc,
            Err(error) => accumulate(acc, error.at_path(&KeyPath::from(key))),
        }
    }

    /// An optional nested section at `key`.
    pub fn option<Context, T>(
        context: &Context,
        key: &'static str,
        value: Option<&T>,
        acc: Option<Context::Error>,
    ) -> Option<Context::Error>
    where
        Context: CanValidateConfig<T>,
        Context::Error: AggregateError,
    {
        match value {
            Some(value) => single(context, key, value, acc),
            None => acc,
        }
    }

    /// A sequence of nested sections at `key`, path `key[i]`.
    pub fn seq<'a, Context, T: 'a>(
        context: &Context,
        key: &'static str,
        values: impl IntoIterator<Item = &'a T>,
        mut acc: Option<Context::Error>,
    ) -> Option<Context::Error>
    where
        Context: CanValidateConfig<T>,
        Context::Error: AggregateError,
    {
        for (index, value) in values.into_iter().enumerate() {
            if let Err(error) = context.validate_config(value) {
                let path = KeyPath::from(key).join(PathSegment::from(index));
                acc = accumulate(acc, error.at_path(&path));
            }
        }
        acc
    }

    /// A map of nested sections at `key`, path `key.<name>`.
    pub fn map<'a, Context, K, T: 'a>(
        context: &Context,
        key: &'static str,
        entries: impl IntoIterator<Item = (&'a K, &'a T)>,
        mut acc: Option<Context::Error>,
    ) -> Option<Context::Error>
    where
        K: AsRef<str> + ?Sized + 'a,
        Context: CanValidateConfig<T>,
        Context::Error: AggregateError,
    {
        for (name, value) in entries {
            if let Err(error) = context.validate_config(value) {
                let path = KeyPath::from(key).join(name.as_ref());
                acc = accumulate(acc, error.at_path(&path));
            }
        }
        acc
    }

    /// The final result of a fold.
    ///
    /// # Errors
    ///
    /// The folded error, when there is one.
    pub fn finish<E>(acc: Option<E>) -> Result<(), E> {
        acc.map_or(Ok(()), Err)
    }
}
