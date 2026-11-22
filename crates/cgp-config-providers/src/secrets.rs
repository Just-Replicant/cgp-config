//! `SecretsPreset` and `MockSecretsPreset` (feature `secrets`).
//!
//! `TestConfigPreset` stays without a secret field. A test that wants stand-ins opts into
//! `MockSecretsPreset` and adds a `secret_map` field. `Debug` of that map prints names only.

#![allow(missing_docs)]

use std::path::Path;
use std::sync::Arc;

use cgp_config_core::{
    EnvSnapshot, HasEnvSnapshot, HasLoadLimits, SecretGuard, SecretSessionComponent,
    SecretSessionOpener,
};
use cgp_config_secrets::{HasSecretRoots, SecretRoots, SecretRootsComponent, StdSecretReader};
use cgp_fork::prelude::*;

use crate::location::HasConfigLocation;

/// Computes secret roots from the config directory, `CREDENTIALS_DIRECTORY`, and
/// `/run/secrets`.
pub struct StdSecretRoots;

#[cgp_impl(StdSecretRoots: SecretRootsComponent)]
impl cgp_config_secrets::SecretRootsGetter
where
    Self: HasConfigLocation + HasEnvSnapshot,
{
    fn secret_roots(&self) -> SecretRoots {
        let dir = self.config_location().dir().map(|dir| {
            if dir.as_os_str().is_empty() {
                Path::new(".")
            } else {
                dir
            }
        });
        SecretRoots::discover(dir, self.env_snapshot())
    }
}

/// Installs [`StdSecretReader`] for the deserialize that follows.
pub struct OpenStdSession;

#[cgp_impl(OpenStdSession: SecretSessionComponent)]
impl SecretSessionOpener
where
    Self: HasEnvSnapshot + HasLoadLimits + HasSecretRoots,
{
    fn open_secret_session(&self) -> SecretGuard {
        let reader = StdSecretReader::new(
            EnvSnapshot::clone(self.env_snapshot()),
            self.secret_roots(),
            self.load_limits().max_bytes,
        );
        SecretGuard::resolve(Arc::new(reader))
    }
}

#[cfg(all(test, feature = "toml"))]
mod tests {
    use cgp_config_core::{
        ConfigError, ConfigLoaded, ConfigMeta, ConfigMetaField, ConfigSection, EnvSnapshot,
        FieldMeta, LoadReport, NoValidation, ValidateSections,
    };
    use cgp_config_secrets::Secret;
    use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
    use cgp_fork::extra::error::RaiseFrom;
    use cgp_fork::prelude::*;
    use serde::Deserialize;

    use crate::loader::{from_toml_str, from_toml_str_with};
    use crate::location::ConfigLocation;
    use crate::presets::__entries::*;
    use crate::presets::{SecretsPreset, TwelveFactorPreset};

    const SENTINEL: &str = "s3cr3t-SENTINEL";

    const APP_FIELDS: &[FieldMeta] = &[
        FieldMeta::new("name", "name"),
        FieldMeta::new("password", "password").with_secret(true),
    ];

    #[derive(Debug, Deserialize)]
    struct App {
        name: String,
        password: Secret<String>,
    }

    impl ConfigSection for App {
        const META: &'static ConfigMeta = &ConfigMeta::new("App", APP_FIELDS);
    }

    const LIT_FIELDS: &[FieldMeta] = &[FieldMeta::new("password", "password").with_secret(true)];

    #[derive(Debug, Deserialize)]
    struct Lit {
        password: Secret<String>,
    }

    impl ConfigSection for Lit {
        const META: &'static ConfigMeta = &ConfigMeta::new("Lit", LIT_FIELDS);
    }

    impl ValidateSections<crate::loader::MemoryLoader> for Lit {
        fn validate_sections(
            &self,
            _context: &crate::loader::MemoryLoader,
        ) -> Result<(), ConfigError> {
            Ok(())
        }
    }

    impl ValidateSections<Loader> for App {
        fn validate_sections(&self, _context: &Loader) -> Result<(), ConfigError> {
            Ok(())
        }
    }

    #[derive(HasField)]
    struct Loader {
        env_snapshot: EnvSnapshot,
        env_prefix: String,
        active_profile: String,
        config_location: ConfigLocation,
        config_meta: Option<&'static ConfigMeta>,
        load_report: LoadReport,
    }

    delegate_components! {
        #[unchecked]
        Loader {
            preset SecretsPreset,
            ErrorTypeProviderComponent: UseType<ConfigError>,
            ErrorRaiserComponent: RaiseFrom,
            RootConfigMetaComponent: ConfigMetaField,
            ConfigValidatorComponent: NoValidation,
        }
    }

    #[test]
    fn redaction_covers_debug_display_explain_dump_and_the_load_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.toml");
        std::fs::write(
            &path,
            "name = \"app\"\npassword = { secret = \"env:DB_PASSWORD\" }\n",
        )
        .unwrap();
        let loader = Loader {
            env_snapshot: EnvSnapshot::from_iter([("DB_PASSWORD", SENTINEL)]),
            env_prefix: String::from("APP"),
            active_profile: String::from("default"),
            config_location: ConfigLocation::explicit(&path),
            config_meta: Some(App::META),
            load_report: LoadReport::new(),
        };
        let app = CanLoadConfig::<App>::load_config(&loader).unwrap();
        assert_eq!(app.name, "app");
        assert_eq!(app.password.expose_secret(), SENTINEL);
        let debug = format!("{app:?}");
        assert!(!debug.contains(SENTINEL), "{debug}");
        assert!(!format!("{}", app.password).contains(SENTINEL));
        let stamp = loader.load_report.stamp().unwrap();
        let loaded = ConfigLoaded::new(stamp.origin.clone(), stamp.fingerprint);
        let log = loaded.to_string();
        assert!(log.contains("fingerprint"), "{log}");
        assert!(!log.contains(SENTINEL), "{log}");
        let explained = stamp.explain("password").unwrap().to_string();
        assert!(explained.contains("[REDACTED]"), "{explained}");
        assert!(!explained.contains(SENTINEL), "{explained}");
        assert!(!stamp.dump_json.contains(SENTINEL), "{}", stamp.dump_json);
        assert!(!format!("{:?}", loader.load_report).contains(SENTINEL));
    }

    #[test]
    fn from_toml_str_keeps_literals_and_refuses_references() {
        let literal: Lit = from_toml_str("password = \"s3cr3t-SENTINEL\"\n").unwrap();
        assert_eq!(literal.password.expose_secret(), SENTINEL);
        assert!(!format!("{literal:?}").contains(SENTINEL));
        let error =
            from_toml_str::<Lit>("password = { secret = \"env:DB_PASSWORD\" }\n").unwrap_err();
        assert_eq!(error.code(), "config::secrets_denied");
        assert!(!error.to_string().contains("DB_PASSWORD"));
        let loaded =
            from_toml_str_with::<Lit>("password = \"s3cr3t-SENTINEL\"\n", |_| Ok(())).unwrap();
        assert!(loaded.fingerprint().is_some());
        assert!(!loaded.dump_json().unwrap().contains(SENTINEL));
        let _ = loaded.explain("password");
    }
}
