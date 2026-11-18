/// Compile-time check of a loader context's wiring, with real parameters.
///
/// Tables that wire parametric components (`CanLoadSource<Source>`,
/// `CanValidateConfig<Config>`, ...) are marked `#[unchecked]`, because the check
/// `delegate_components!` generates uses `()` as every parameter and fails for those
/// components (ADR-001). `config_check!` is the supported replacement: it expands to
/// `check_components!` for the loader, deserializer, interpolator, secrets, and validator
/// components at `config`, and the source and preparer components at every listed source.
///
/// The source markers below are placeholders; the facade crate's documentation has a
/// complete, compiled example.
///
/// ```ignore
/// config_check! {
///     Loader {
///         config: AppConfig,
///         sources: [Defaults, ConfigFile, ProfileFile, EnvVars],
///     }
/// }
/// ```
///
/// A context checked more than once in the same module names each check trait:
/// `#[check_trait(CanLoadAppConfig)] Loader { .. }`.
///
/// Like every CGP wiring macro, the expansion names the fork crate, so the calling crate
/// depends on `cgp-fork` directly (ADR-001, finding 6).
#[macro_export]
macro_rules! config_check {
    (
        $( #[check_trait($check_trait:ident)] )?
        $context:ident {
            config: $config:ty,
            sources: [ $( $source:ty ),* $(,)? ] $(,)?
        }
    ) => {
        $crate::cgp::prelude::check_components! {
            $( #[check_trait($check_trait)] )?
            $context {
                $crate::components::ConfigLoaderComponent: $config,
                $crate::components::InterpolatorComponent: $config,
                $crate::components::ConfigDeserializerComponent: $config,
                $crate::components::SecretsResolverComponent: $config,
                $crate::components::ConfigValidatorComponent: $config,
                $crate::components::SourceLoaderComponent: [ $( $source ),* ],
                $crate::components::LayerPreparerComponent: [ $( $source ),* ],
                $crate::components::LayerMergerComponent,
                $crate::components::EmptyConfigValueComponent,
            }
        }
    };
}
