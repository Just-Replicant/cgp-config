//! The garde and validator adapters report every problem in one `ConfigError`.

#![cfg(all(feature = "validate-garde", feature = "validate-validator"))]
#![allow(missing_docs)]

use cgp_config_core::ConfigError;
use cgp_config_core::components::*;
use cgp_config_providers::validate::{ValidateWithGarde, ValidateWithValidator};
use cgp_fork::core::error::{ErrorRaiserComponent, ErrorTypeProviderComponent};
use cgp_fork::extra::error::RaiseFrom;
use cgp_fork::prelude::*;
// The validator derive calls `.validate()` on nested fields.
use validator::Validate as _;

#[derive(garde::Validate)]
struct GardeHttp {
    #[garde(range(min = 1024))]
    port: u16,
}

#[derive(garde::Validate)]
struct GardeApp {
    #[garde(length(min = 1))]
    name: String,
    #[garde(dive)]
    http: GardeHttp,
}

#[derive(validator::Validate)]
struct VHttp {
    #[validate(range(min = 1024))]
    port: u16,
}

#[derive(validator::Validate)]
struct VApp {
    #[validate(length(min = 1, message = "must not be empty"))]
    name: String,
    #[validate(nested)]
    http: VHttp,
}

struct GardeCtx;
struct ValidatorCtx;

delegate_components! {
    #[unchecked]
    GardeCtx {
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        ConfigValidatorComponent: ValidateWithGarde,
    }
}

delegate_components! {
    #[unchecked]
    ValidatorCtx {
        ErrorTypeProviderComponent: UseType<ConfigError>,
        ErrorRaiserComponent: RaiseFrom,
        ConfigValidatorComponent: ValidateWithValidator,
    }
}

fn paths(error: &ConfigError) -> Vec<String> {
    let ConfigError::Validation(issues) = error else {
        panic!("expected validation, got {error}");
    };
    let mut paths: Vec<String> = issues.iter().map(|issue| issue.path.to_string()).collect();
    paths.sort();
    paths
}

#[test]
fn garde_issues_are_aggregated() {
    let bad = GardeApp {
        name: String::new(),
        http: GardeHttp { port: 80 },
    };
    let error = GardeCtx.validate_config(&bad).unwrap_err();
    assert_eq!(paths(&error), ["http.port", "name"]);
    let good = GardeApp {
        name: "x".into(),
        http: GardeHttp { port: 8080 },
    };
    GardeCtx.validate_config(&good).unwrap();
}

#[test]
fn validator_issues_are_aggregated() {
    let bad = VApp {
        name: String::new(),
        http: VHttp { port: 80 },
    };
    let error = ValidatorCtx.validate_config(&bad).unwrap_err();
    assert_eq!(paths(&error), ["http.port", "name"]);
    assert!(error.to_string().contains("must not be empty"), "{error}");
    let good = VApp {
        name: "x".into(),
        http: VHttp { port: 8080 },
    };
    ValidatorCtx.validate_config(&good).unwrap();
}
