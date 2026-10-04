//! Resolve explicit Cargo feature policy into package-local semantic inputs.

use std::collections::BTreeSet;

use velnor_actions_contract::{RustConfiguration, RustFeatureMode};
use velnor_actions_rust::PackageRecord;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::derive_groups::FeatureFallback;

/// Resolve all-features before task identity and argv construction.
///
/// Cargo's all-features enables every declared feature, including `default`.
/// Passing that complete list with no implicit defaults is equivalent and
/// keeps identities bound to the actual feature graph. A real feature named
/// `all` remains an ordinary selected feature.
pub(crate) fn resolve(
    package: &PackageRecord,
    config: &RustConfiguration,
    union: &BTreeSet<String>,
) -> Result<(Vec<String>, Option<FeatureFallback>), OrchestratorError> {
    if config.feature_mode == RustFeatureMode::All {
        let mut features = package.features.clone();
        features.sort();
        features.dedup();
        return Ok((features, None));
    }
    let (mut features, mut fallback) = selected(package, &config.features, union, &config.name)?;
    if config.feature_mode == RustFeatureMode::DefaultAndSelected
        && package.features.iter().any(|name| name == "default")
    {
        features.push("default".to_owned());
        features.sort();
        features.dedup();
    }
    if let Some(fallback) = &mut fallback {
        fallback.applied.clone_from(&features);
    }
    Ok((features, fallback))
}

fn selected(
    package: &PackageRecord,
    requested: &[String],
    union: &BTreeSet<String>,
    configuration: &str,
) -> Result<(Vec<String>, Option<FeatureFallback>), OrchestratorError> {
    if requested.is_empty() || matches!(requested, [only] if only == "default") {
        return Ok((requested.to_vec(), None));
    }
    for feature in requested {
        if !union.contains(feature) {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                "stacks.rust.configurations.features",
                format!("unknown_feature:{configuration}:{}:{feature}", package.name),
            ));
        }
    }
    let declared: BTreeSet<&str> = package.features.iter().map(String::as_str).collect();
    let mut applied: Vec<String> = requested
        .iter()
        .filter(|name| declared.contains(name.as_str()))
        .cloned()
        .collect();
    applied.sort();
    applied.dedup();
    let mut sorted = requested.to_vec();
    sorted.sort();
    sorted.dedup();
    if applied == sorted {
        return Ok((applied, None));
    }
    if applied.is_empty() {
        applied.push("default".to_owned());
    }
    let fallback = FeatureFallback {
        package_name: package.name.clone(),
        configuration: configuration.to_owned(),
        requested: sorted,
        applied: applied.clone(),
    };
    Ok((applied, Some(fallback)))
}

#[cfg(test)]
#[path = "rust_features_tests.rs"]
mod tests;
