//! Compare supplied rustdoc using native Cargo package and lint context.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use cargo_metadata::{Metadata, Package};
use serde::{Deserialize, Serialize};
use trustfall_rustdoc::VersionedStorage;

use crate::check_release::{CheckReleaseSettings, run_check_release};
use crate::data_generation::DataStorage;
use crate::{GlobalConfig, ReleaseType, Report, RustdocIndexingMode, WitnessGeneration};

/// Exact Cargo package identity and the metadata document containing it.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageContext {
    pub metadata_path: PathBuf,
    pub package_id: String,
    pub package_name: String,
    pub package_version: semver::Version,
    pub manifest_path: PathBuf,
}

/// Rustdoc paired with its exact Cargo package.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuppliedDocs {
    pub package: PackageContext,
    pub rustdoc_path: PathBuf,
}

/// Explicit release classification passed directly to the native checker.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SuppliedReleaseType {
    Major,
    Minor,
    Patch,
}

impl From<&SuppliedReleaseType> for ReleaseType {
    fn from(value: &SuppliedReleaseType) -> Self {
        match value {
            SuppliedReleaseType::Major => Self::Major,
            SuppliedReleaseType::Minor => Self::Minor,
            SuppliedReleaseType::Patch => Self::Patch,
        }
    }
}

/// Closed request for a single native package comparison.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuppliedCompareRequest {
    pub schema_version: u32,
    pub current: SuppliedDocs,
    pub baseline: SuppliedDocs,
    pub current_workspace_manifest_path: PathBuf,
    pub release_type: SuppliedReleaseType,
}

/// Execute native checks without rebuilding either rustdoc document.
pub fn compare_supplied(
    config: &mut GlobalConfig,
    request: &SuppliedCompareRequest,
) -> Result<Report> {
    ensure!(
        request.schema_version == 1,
        "unsupported comparison request schema"
    );
    let (metadata, current) = load_package_context(&request.current.package)?;
    let (_, baseline) = load_package_context(&request.baseline.package)?;
    ensure!(
        current.name == baseline.name,
        "baseline/current package names differ"
    );
    validate_workspace_manifest(&metadata, &request.current_workspace_manifest_path)?;
    let workspace_overrides = crate::manifest::deserialize_lint_table(&metadata.workspace_metadata)
        .context("[workspace.metadata.cargo-semver-checks] table is invalid")?
        .map(|table| table.into_stack());
    let overrides =
        crate::overrides_for_workspace_package(&current, workspace_overrides.as_deref())?;
    let current_storage = load_package_rustdoc(&request.current.rustdoc_path, current.clone())?;
    let baseline_storage = load_package_rustdoc(&request.baseline.rustdoc_path, baseline)?;
    ensure!(
        current_storage.version() == baseline_storage.version(),
        "baseline/current rustdoc format versions differ"
    );
    let data = DataStorage::new(current_storage, baseline_storage);
    let pending = run_check_release(
        config,
        &data,
        current.name.as_str(),
        CheckReleaseSettings {
            release_type: Some((&request.release_type).into()),
            rustdoc_indexing_mode: RustdocIndexingMode::Ordinary,
        },
        &overrides,
        &WitnessGeneration::default(),
        crate::witness_gen::WitnessGenerationData::new(
            None,
            None,
            metadata.target_directory.clone().into_std_path_buf(),
        ),
    )?;
    crate::witness_gen::finalize_retained_artifacts(
        config.run_id(),
        &[pending.witness_run_report],
    )?;
    Ok(Report {
        crate_reports: BTreeMap::from([(current.name.to_string(), pending.report)]),
    })
}

pub(crate) fn load_package_context(context: &PackageContext) -> Result<(Metadata, Package)> {
    require_absolute(&context.metadata_path)?;
    require_absolute(&context.manifest_path)?;
    let metadata: Metadata = serde_json::from_slice(&fs_err::read(&context.metadata_path)?)
        .with_context(|| format!("invalid Cargo metadata {}", context.metadata_path.display()))?;
    let mut selected = metadata
        .packages
        .iter()
        .filter(|package| package.id.repr == context.package_id);
    let package = selected
        .next()
        .context("exact package ID absent from Cargo metadata")?;
    ensure!(
        selected.next().is_none(),
        "duplicate package ID in Cargo metadata"
    );
    ensure!(
        package.name.as_str() == context.package_name,
        "package name mismatch"
    );
    ensure!(
        package.version == context.package_version,
        "package version mismatch"
    );
    ensure!(
        package.manifest_path.as_std_path() == context.manifest_path,
        "manifest path mismatch"
    );
    ensure!(
        metadata.workspace_members.contains(&package.id),
        "selected package is not a workspace member"
    );
    ensure!(
        package
            .targets
            .iter()
            .any(crate::is_lib_like_checkable_target),
        "selected package has no checkable library target"
    );
    let manifest = crate::manifest::Manifest::parse(context.manifest_path.clone())?;
    ensure!(
        crate::manifest::get_package_name(&manifest)? == context.package_name,
        "actual manifest name mismatch"
    );
    ensure!(
        semver::Version::parse(&crate::manifest::get_package_version(&manifest)?)?
            == context.package_version,
        "actual manifest version mismatch"
    );
    validate_manifest_metadata(&context.manifest_path, &package.metadata, false)?;
    let package = package.clone();
    Ok((metadata, package))
}

pub(crate) fn validate_workspace_manifest(metadata: &Metadata, path: &Path) -> Result<()> {
    require_absolute(path)?;
    ensure!(
        metadata.workspace_root.join("Cargo.toml").as_std_path() == path,
        "workspace manifest path mismatch"
    );
    validate_manifest_metadata(path, &metadata.workspace_metadata, true)
}

fn validate_manifest_metadata(
    path: &Path,
    expected: &serde_json::Value,
    workspace: bool,
) -> Result<()> {
    let manifest: cargo_toml::Manifest<serde_json::Value> =
        cargo_toml::Manifest::from_slice_with_metadata(&fs_err::read(path)?)?;
    let actual = if workspace {
        manifest.workspace.and_then(|table| table.metadata)
    } else {
        manifest.package.and_then(|table| table.metadata)
    };
    ensure!(
        actual.unwrap_or(serde_json::Value::Null) == *expected,
        "Cargo metadata differs from actual manifest metadata at {}",
        path.display()
    );
    Ok(())
}

fn require_absolute(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute(),
        "supplied path must be absolute: {}",
        path.display()
    );
    Ok(())
}

fn load_package_rustdoc(path: &Path, package: Package) -> Result<VersionedStorage> {
    require_absolute(path)?;
    let bytes = fs_err::read(path)?;
    #[derive(Deserialize)]
    struct FormatVersion {
        format_version: u32,
    }
    let version: FormatVersion = serde_json::from_slice(&bytes)?;
    macro_rules! load_version {
        ($adapter:ident, $variant:ident) => {{
            let rustdoc: $adapter::Crate = serde_json::from_slice(&bytes)
                .with_context(|| format!("invalid rustdoc document {}", path.display()))?;
            ensure!(
                rustdoc.crate_version.as_deref() == Some(package.version.to_string().as_str()),
                "rustdoc crate version differs from selected package"
            );
            let root = rustdoc
                .index
                .get(&rustdoc.root)
                .context("rustdoc root item missing")?;
            ensure!(
                package
                    .targets
                    .iter()
                    .filter(|target| crate::is_lib_like_checkable_target(target))
                    .any(|target| root.name.as_deref() == Some(target.name.as_str())),
                "rustdoc root name differs from selected library target"
            );
            Ok(VersionedStorage::$variant(
                $adapter::PackageStorage::from_rustdoc_and_package(rustdoc, package),
            ))
        }};
    }
    match version.format_version {
        57 => load_version!(trustfall_rustdoc_adapter_v57, V57),
        60 => load_version!(trustfall_rustdoc_adapter_v60, V60),
        61 => load_version!(trustfall_rustdoc_adapter_v61, V61),
        other => anyhow::bail!("unsupported rustdoc format {other}; supported: 57, 60, 61"),
    }
}
