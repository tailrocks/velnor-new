//! Native feature planning for externally acquired, authenticated source artifacts.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, ensure};
use serde::{Deserialize, Serialize};

use crate::GlobalConfig;
use crate::data_generation::{RustdocBuildEnvironment, TerminalError, determine_rustdoc_dir};
use crate::manifest::Manifest;
use crate::rustdoc_gen::{
    CrateDataForRustdoc, CrateSource, CrateType, FeatureConfig, FeaturesGroup,
    generate_data_request,
};
use crate::supplied_compare::{PackageContext, load_package_context};

/// Closed feature selection accepted by the supplied-artifact planner.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuppliedFeatureGroup {
    All,
    Default,
    #[default]
    Heuristic,
    None,
}

impl SuppliedFeatureGroup {
    fn native(self) -> FeaturesGroup {
        match self {
            Self::All => FeaturesGroup::All,
            Self::Default => FeaturesGroup::Default,
            Self::Heuristic => FeaturesGroup::Heuristic,
            Self::None => FeaturesGroup::None,
        }
    }
}

/// Both metadata documents must have been acquired by the calling source owner.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuppliedPlanRequest {
    pub schema_version: u32,
    pub current: PackageContext,
    pub baseline: PackageContext,
    pub baseline_index_version_path: PathBuf,
    #[serde(default)]
    pub feature_group: SuppliedFeatureGroup,
    #[serde(default)]
    pub extra_current_features: Vec<String>,
    #[serde(default)]
    pub extra_baseline_features: Vec<String>,
    #[serde(default)]
    pub target: Option<String>,
}

/// Explicit features reflect the native effective request's ordering and deduplication.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuppliedPackagePlan {
    pub package: PackageContext,
    pub features: Vec<String>,
    pub use_default_features: bool,
    pub target: Option<String>,
    pub rustdoc_relative_directory: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuppliedPlan {
    pub schema_version: u32,
    pub current: SuppliedPackagePlan,
    pub baseline: SuppliedPackagePlan,
    pub baseline_index_version_path: PathBuf,
    pub target: Option<String>,
    pub build_environment: SuppliedBuildEnvironment,
}

/// Cargo configuration and flags resolved by the native rustdoc environment owner.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SuppliedBuildEnvironment {
    pub target_triple: String,
    pub cargo_rustflags: String,
    pub cargo_rustdocflags: String,
    pub toolchain_version: String,
}

/// Resolve native source, build environment, and output-directory policies.
pub fn plan_supplied(
    config: &mut GlobalConfig,
    request: &SuppliedPlanRequest,
) -> anyhow::Result<SuppliedPlan> {
    ensure!(
        request.schema_version == 1,
        "unsupported plan request schema"
    );
    load_package_context(&request.current)?;
    load_package_context(&request.baseline)?;
    ensure!(
        request.current.package_name == request.baseline.package_name,
        "baseline/current package names differ"
    );
    let build_environment =
        RustdocBuildEnvironment::from_env_and_config_for_target(request.target.as_deref())?;
    let current_manifest = Manifest::parse(request.current.manifest_path.clone())?;
    let baseline_index_version = load_index_version(request)?;
    let current_source = CrateSource::ManifestPath {
        manifest: &current_manifest,
    };
    let baseline_source = CrateSource::Registry {
        versioned_krate: &baseline_index_version,
    };
    let current = package_plan(
        config,
        &request.current,
        current_source,
        FeatureConfig {
            features_group: request.feature_group.native(),
            extra_features: request.extra_current_features.clone(),
            is_baseline: false,
        },
        request.target.as_deref(),
    )?;
    let baseline = package_plan(
        config,
        &request.baseline,
        baseline_source,
        FeatureConfig {
            features_group: request.feature_group.native(),
            extra_features: request.extra_baseline_features.clone(),
            is_baseline: true,
        },
        request.target.as_deref(),
    )?;
    Ok(SuppliedPlan {
        schema_version: 1,
        current,
        baseline,
        baseline_index_version_path: request.baseline_index_version_path.clone(),
        target: request.target.clone(),
        build_environment: SuppliedBuildEnvironment {
            target_triple: build_environment.target_triple,
            cargo_rustflags: build_environment.cargo_rustflags.into_owned(),
            cargo_rustdocflags: build_environment.cargo_rustdocflags.into_owned(),
            toolchain_version: build_environment.toolchain_version,
        },
    })
}

fn load_index_version(request: &SuppliedPlanRequest) -> anyhow::Result<tame_index::IndexVersion> {
    let path = &request.baseline_index_version_path;
    ensure!(path.is_absolute(), "registry entry path must be absolute");
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read registry entry {}", path.display()))?;
    let version: tame_index::IndexVersion = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse registry entry {}", path.display()))?;
    ensure!(
        version.name.as_str() == request.baseline.package_name,
        "registry entry package name differs from baseline package"
    );
    ensure!(
        version.version.as_str() == request.baseline.package_version.to_string(),
        "registry entry package version differs from baseline package"
    );
    Ok(version)
}

fn package_plan(
    config: &mut GlobalConfig,
    package: &PackageContext,
    source: CrateSource<'_>,
    feature_config: FeatureConfig,
    target: Option<&str>,
) -> anyhow::Result<SuppliedPackagePlan> {
    let crate_data = CrateDataForRustdoc {
        crate_type: if feature_config.is_baseline {
            CrateType::Baseline {
                highest_allowed_version: None,
            }
        } else {
            CrateType::Current
        },
        name: package.package_name.clone(),
        feature_config: &feature_config,
        build_target: target,
    };
    let request = generate_data_request(config, source, &crate_data);
    let rustdoc_relative_directory = determine_rustdoc_dir(
        &request,
        Path::new(""),
        &package.package_name,
        &package.package_version.to_string(),
    )
    .map_err(|error| match error {
        TerminalError::WithAdvice(error, advice) => error.context(advice),
        TerminalError::Other(error) => error,
    })?;
    Ok(SuppliedPackagePlan {
        package: package.clone(),
        features: request.extra_features().map(str::to_owned).collect(),
        use_default_features: request.default_features_enabled(),
        target: request.build_target().map(str::to_owned),
        rustdoc_relative_directory,
    })
}
