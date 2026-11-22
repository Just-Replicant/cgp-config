//! The core pipeline with a value type of its own: the components do not assume
//! `ConfigValue`, serde, or any particular merge.

// Tests run on std; the core/alloc import lints guard the no_std library code only.
#![allow(clippy::std_instead_of_core, clippy::std_instead_of_alloc)]

use std::cell::RefCell;

use cgp_config_core::components::*;
use cgp_config_core::{
    CollectInOrder, ConfigError, DefaultLoadLimits, DenySecrets, IdentityLayer, IgnoreWarnings,
    KeyPath, LoadLayered, LoadLimits, LoadLimitsField, LoadReport, LoadWarning, NoInterpolation,
    NoSecretSession, NoValidation, ReportOnLoader, StaticKeyPath, config_check,
};
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;
use cgp_fork::prelude::*;

/// An association list: later entries win.
#[derive(Clone, Debug, Default, PartialEq)]
struct Pairs(Vec<(&'static str, i64)>);

impl Pairs {
    fn get(&self, key: &str) -> Option<i64> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
    }
}

#[derive(Debug, PartialEq)]
struct Limits {
    workers: i64,
    retries: i64,
}

struct First;
struct Second;
struct Absent;

struct PairSources;

#[cgp_impl(PairSources)]
impl SourceLoader<First>
where
    Self: HasConfigValueType<ConfigValue = Pairs> + HasErrorType,
{
    fn load_source(&self, _: PhantomData<First>) -> Result<Option<Pairs>, Self::Error> {
        Ok(Some(Pairs(vec![("workers", 1), ("retries", 3)])))
    }
}

#[cgp_impl(PairSources)]
impl SourceLoader<Second>
where
    Self: HasConfigValueType<ConfigValue = Pairs> + HasErrorType,
{
    fn load_source(&self, _: PhantomData<Second>) -> Result<Option<Pairs>, Self::Error> {
        Ok(Some(Pairs(vec![("workers", 8)])))
    }
}

#[cgp_impl(PairSources)]
impl SourceLoader<Absent>
where
    Self: HasConfigValueType<ConfigValue = Pairs> + HasErrorType,
{
    fn load_source(&self, _: PhantomData<Absent>) -> Result<Option<Pairs>, Self::Error> {
        Ok(None)
    }
}

struct EmptyPairs;

#[cgp_impl(EmptyPairs)]
impl EmptyConfigValue
where
    Self: HasConfigValueType<ConfigValue = Pairs>,
{
    fn empty_config_value(&self) -> Pairs {
        Pairs::default()
    }
}

/// Concatenation, and a count of merges in the context for the test to read.
struct ConcatPairs;

#[cgp_impl(ConcatPairs)]
impl LayerMerger
where
    Self: HasConfigValueType<ConfigValue = Pairs>
        + HasErrorType
        + HasField<Symbol!("merges"), Value = RefCell<usize>>
        + HasLoadLimits,
{
    fn merge_layers(&self, mut base: Pairs, overlay: Pairs) -> Result<Pairs, Self::Error> {
        *self
            .get_field(PhantomData::<Symbol!("merges")>)
            .borrow_mut() += 1;
        base.0.extend(overlay.0);
        assert!(base.0.len() <= self.load_limits().max_keys);
        Ok(base)
    }
}

struct PairsToLimits;

#[cgp_impl(PairsToLimits)]
impl ConfigDeserializer<Limits>
where
    Self: HasConfigValueType<ConfigValue = Pairs> + CanRaiseError<ConfigError> + CanReportWarning,
{
    fn deserialize_config(&self, value: Pairs) -> Result<Limits, Self::Error> {
        let field = |name: &'static str| {
            value
                .get(name)
                .ok_or_else(|| Self::raise_error(ConfigError::missing(name, "integer")))
        };
        for (key, _) in &value.0 {
            if !["workers", "retries"].contains(key) {
                self.report_warning(LoadWarning::UnknownKey {
                    path: KeyPath::from(*key),
                    suggestion: None,
                    meta: None,
                });
            }
        }
        Ok(Limits {
            workers: field("workers")?,
            retries: field("retries")?,
        })
    }
}

#[derive(HasField)]
struct Loader {
    merges: RefCell<usize>,
    load_report: LoadReport,
    load_limits: LoadLimits,
}

delegate_components! {
    #[unchecked]
    Loader {
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        ConfigValueTypeProviderComponent: UseType<Pairs>,
        EmptyConfigValueComponent: EmptyPairs,
        SourceLoaderComponent: PairSources,
        LayerPreparerComponent: IdentityLayer,
        LayerMergerComponent: ConcatPairs,
        LayerCollectorComponent: CollectInOrder,
        InterpolatorComponent: NoInterpolation,
        ConfigDeserializerComponent: PairsToLimits,
        SecretsResolverComponent: DenySecrets,
        SecretSessionComponent: NoSecretSession,
        ConfigValidatorComponent: NoValidation,
        LoadLimitsComponent: LoadLimitsField,
        LoadReportComponent: ReportOnLoader,
        ConfigLoaderComponent: LoadLayered<Product![First, Absent, Second]>,
    }
}

config_check! {
    Loader {
        config: Limits,
        sources: [First, Absent, Second],
    }
}

fn loader() -> Loader {
    Loader {
        merges: RefCell::new(0),
        load_report: LoadReport::new(),
        load_limits: LoadLimits::DEFAULT,
    }
}

#[test]
fn sources_fold_in_order_and_absent_ones_are_skipped() {
    let loader = loader();
    let limits: Limits = loader.load_config().unwrap();
    assert_eq!(
        limits,
        Limits {
            workers: 8,
            retries: 3
        }
    );
    assert_eq!(*loader.merges.borrow(), 2);
    assert!(loader.load_report().is_empty());
}

#[test]
fn stages_are_callable_one_by_one() {
    let loader = loader();
    let tree = loader
        .collect_layers(
            loader.empty_config_value(),
            PhantomData::<Product![Second, First]>,
        )
        .unwrap();
    assert_eq!(tree.get("workers"), Some(1));
    let tree = CanInterpolate::<Limits>::interpolate(&loader, tree).unwrap();
    let mut extra = tree.clone();
    extra.0.push(("unknown", 0));
    let limits: Limits = loader.deserialize_config(extra).unwrap();
    assert_eq!(loader.load_report().len(), 1);
    let mut limits = limits;
    loader.resolve_secrets(&mut limits).unwrap();
    loader.validate_config(&limits).unwrap();
    let layer = loader
        .prepare_layer(PhantomData::<First>, Pairs::default())
        .unwrap();
    assert_eq!(layer, Pairs::default());
    assert_eq!(loader.load_limits().max_keys, LoadLimits::DEFAULT.max_keys);
}

#[test]
fn each_load_clears_the_previous_report() {
    let loader = loader();
    loader.report_warning(LoadWarning::MissingProfile {
        profile: "from an earlier load".into(),
    });
    assert_eq!(loader.load_report().len(), 1);
    let _: Limits = loader.load_config().unwrap();
    assert!(loader.load_report().is_empty());
    loader.report_warning(LoadWarning::MissingProfile {
        profile: "x".into(),
    });
    loader.clear_warnings();
    assert!(loader.load_report().is_empty());
}

#[test]
fn static_paths_convert_to_owned_paths() {
    const PORT: StaticKeyPath = StaticKeyPath::new(&["http", "port"]);
    assert_eq!(PORT.to_string(), "http.port");
    assert_eq!(StaticKeyPath::ROOT.to_string(), "(root)");
    assert_eq!(KeyPath::from(PORT), KeyPath::parse("http.port").unwrap());
}

#[test]
fn reserved_document_keys() {
    use cgp_config_core::reserved;
    assert!(reserved::is_reserved(reserved::PROFILES));
    assert!(!reserved::is_reserved("profiles"));
    assert_eq!(reserved::ALL.len(), 3);
}

#[test]
fn deserializer_errors_surface_unchanged() {
    let loader = loader();
    let error =
        CanDeserializeConfig::<Limits>::deserialize_config(&loader, Pairs(vec![("workers", 1)]))
            .unwrap_err();
    assert_eq!(error.to_string(), "missing retries (expected integer)");
}

/// The same pipeline with other report and limit providers.
#[derive(HasField)]
struct Quiet {
    merges: RefCell<usize>,
}

delegate_components! {
    #[unchecked]
    Quiet {
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        ConfigValueTypeProviderComponent: UseType<Pairs>,
        EmptyConfigValueComponent: EmptyPairs,
        SourceLoaderComponent: PairSources,
        LayerPreparerComponent: IdentityLayer,
        LayerMergerComponent: ConcatPairs,
        LayerCollectorComponent: CollectInOrder,
        InterpolatorComponent: NoInterpolation,
        ConfigDeserializerComponent: PairsToLimits,
        SecretsResolverComponent: DenySecrets,
        SecretSessionComponent: NoSecretSession,
        ConfigValidatorComponent: NoValidation,
        LoadLimitsComponent: DefaultLoadLimits,
        LoadReportComponent: IgnoreWarnings,
        ConfigLoaderComponent: LoadLayered<Product![First]>,
    }
}

config_check! {
    #[check_trait(CanUseQuiet)]
    Quiet {
        config: Limits,
        sources: [First],
    }
}

#[test]
fn default_limits_and_ignored_warnings() {
    let quiet = Quiet {
        merges: RefCell::new(0),
    };
    assert_eq!(quiet.load_limits(), LoadLimits::DEFAULT);
    quiet.report_warning(LoadWarning::MissingProfile {
        profile: "x".into(),
    });
    // `IgnoreWarnings` has nothing to clear; `LoadLayered` still calls the hook.
    quiet.clear_warnings();
    let limits: Limits = quiet.load_config().unwrap();
    assert_eq!(limits.workers, 1);
}
