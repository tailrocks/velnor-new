//! Portable workloads: closed operations and explicit repository inputs.

use super::Utf8RepoRelDir;
use crate::errors::ContractError;
use crate::ids::is_component_byte;
use serde::{Deserialize, Serialize};

/// A portable operation with fixed adapter-owned command arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadKind {
    /// Build the Dockerfile at the workload root.
    DockerBuild,
    /// Run Bun package CI.
    BunCi,
    /// Run explicitly declared npm package CI.
    NodeCi,
    /// Test a Swift package.
    SwiftTest,
    /// Check the syntax of explicitly listed Ruby files.
    RubySyntax,
    /// Check explicitly listed shell files.
    Shellcheck,
    /// Check repository license metadata.
    Reuse,
    /// Check a Gradle project through its verified wrapper.
    GradleCheck,
    /// Run database migration and code-generation checks through the wrapper.
    GradleDatabaseCheck,
    /// Fresh locked dependency advisory audit.
    CargoAudit,
    /// Fresh dependency advisories, bans, licenses and source checks.
    CargoDeny,
    /// Repository shape and line-limit validation.
    Alint,
    /// Tuiscotti's reviewed pty-free dependency graph guard.
    TuiNoDefaultGraph,
    /// Tuiscotti's reviewed source-policy xtask.
    TuiXtaskPolicy,
    /// Tuiscotti's reviewed dependency-policy xtask.
    TuiXtaskDeps,
    /// Tuiscotti's reviewed packaging dry-run xtask.
    TuiXtaskPackage,
    /// Check a declared native Xcode project and its Rust FFI producer.
    NativeXcodeProjectCi,
    /// Check a declared native Swift package and its Rust FFI producer.
    NativeSwiftPackageCi,
    /// Audit Homebrew formulae through the reviewed local tap procedure.
    HomebrewAudit,
    /// Exercise a declared package updater through compiled release fixtures.
    PackageUpdateFixture,
}

/// One named workload; no raw command, shell, or argument escape hatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadConfig {
    /// Safe component used as the workload identity.
    pub name: String,
    /// Closed portable operation.
    pub kind: WorkloadKind,
    /// Repository-relative working directory; defaults to `.`.
    #[serde(default = "default_root")]
    pub root: Utf8RepoRelDir,
    /// Explicit repository-relative input files, sorted and unique.
    #[serde(default)]
    pub inputs: Vec<String>,
    /// Explicit files for Ruby syntax and Shellcheck, sorted and unique.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Closed package scripts, in their compiled validation order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scripts: Option<Vec<super::PackageScript>>,
    /// Closed Gradle project and local database descriptor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradle: Option<super::GradleWorkloadConfig>,
    /// Declared native source and artifact identities for desktop checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_desktop: Option<super::NativeDesktopProfile>,
    /// Source updater and artifact identities for compiled fixture cases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_update: Option<super::PackageUpdateFixture>,
}

fn default_root() -> Utf8RepoRelDir {
    Utf8RepoRelDir::from_raw(".".to_owned())
}

/// True for the canonical workload identity component grammar.
#[must_use]
pub fn is_valid_workload_name(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | "..")
        && !name.starts_with('-')
        && name.bytes().all(is_component_byte)
}

/// True for a normalized, render-safe repository-relative file path.
#[must_use]
pub fn is_valid_workload_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && !segment.starts_with('-')
                && segment.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'+' | b'@')
                })
        })
}

impl WorkloadConfig {
    /// Validate identity, working directory, and explicit file lists.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let key = format!("stacks.workloads.{}", self.name);
        if !is_valid_workload_name(&self.name) {
            return Err(ContractError::config(
                file,
                "stacks.workloads.name",
                "bad_component",
            ));
        }
        let root = self.root.as_str();
        if Utf8RepoRelDir::parse(root).is_err() || (root != "." && !is_valid_workload_path(root)) {
            return Err(ContractError::config(
                file,
                format!("{key}.root"),
                "bad_root",
            ));
        }
        validate_paths(&self.inputs, file, &format!("{key}.inputs"))?;
        validate_paths(&self.paths, file, &format!("{key}.paths"))?;
        if root != "."
            && self
                .paths
                .iter()
                .any(|path| !path.starts_with(&format!("{root}/")))
        {
            return Err(ContractError::config(
                file,
                format!("{key}.paths"),
                "path_outside_workload_root",
            ));
        }
        let explicit_files = matches!(
            self.kind,
            WorkloadKind::RubySyntax | WorkloadKind::Shellcheck
        );
        if explicit_files && self.paths.is_empty() {
            return Err(ContractError::config(
                file,
                format!("{key}.paths"),
                "empty_paths",
            ));
        }
        if !explicit_files && !self.paths.is_empty() {
            return Err(ContractError::config(
                file,
                format!("{key}.paths"),
                "unexpected_paths",
            ));
        }
        self.validate_gradle(file)?;
        self.validate_native_desktop(file)?;
        self.validate_package_update(file)?;
        self.validate_package(file)
    }

    fn validate_package_update(&self, file: &str) -> Result<(), ContractError> {
        let key = format!("stacks.workloads.{}.package_update", self.name);
        match (
            &self.package_update,
            self.kind == WorkloadKind::PackageUpdateFixture,
        ) {
            (None, false) => Ok(()),
            (None, true) => Err(ContractError::config(
                file,
                key,
                "missing_package_update_fixture",
            )),
            (Some(_), false) => Err(ContractError::config(
                file,
                key,
                "unexpected_package_update_fixture",
            )),
            (Some(profile), true) => {
                if self.root.as_str() != "." {
                    return Err(ContractError::config(
                        file,
                        key,
                        "package_update_requires_repository_root",
                    ));
                }
                profile.validate(file, &key)
            }
        }
    }

    fn validate_native_desktop(&self, file: &str) -> Result<(), ContractError> {
        let key = format!("stacks.workloads.{}.native_desktop", self.name);
        let native = matches!(
            self.kind,
            WorkloadKind::NativeXcodeProjectCi | WorkloadKind::NativeSwiftPackageCi
        );
        match (&self.native_desktop, native) {
            (None, false) => Ok(()),
            (None, true) => Err(ContractError::config(
                file,
                key,
                "missing_native_desktop_profile",
            )),
            (Some(_), false) => Err(ContractError::config(
                file,
                key,
                "unexpected_native_desktop_profile",
            )),
            (Some(profile), true) => {
                profile.validate(file, &key)?;
                let xcode = self.kind == WorkloadKind::NativeXcodeProjectCi;
                if xcode != profile.apple.is_some() {
                    return Err(ContractError::config(
                        file,
                        key,
                        "native_profile_kind_mismatch",
                    ));
                }
                Ok(())
            }
        }
    }
}

fn validate_paths(paths: &[String], file: &str, key: &str) -> Result<(), ContractError> {
    if paths.windows(2).any(|pair| pair[0] > pair[1]) {
        return Err(ContractError::config(file, key, "must_be_sorted"));
    }
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(ContractError::config(file, key, "duplicate_path"));
    }
    if paths.iter().any(|path| !is_valid_workload_path(path)) {
        return Err(ContractError::config(file, key, "bad_path"));
    }
    Ok(())
}

/// Validate workload ordering and each closed operation.
/// # Errors
pub(super) fn validate_workloads(
    workloads: &[WorkloadConfig],
    file: &str,
) -> Result<(), ContractError> {
    if workloads.windows(2).any(|pair| pair[0].name > pair[1].name) {
        return Err(ContractError::config(
            file,
            "stacks.workloads",
            "must_be_sorted",
        ));
    }
    if workloads
        .windows(2)
        .any(|pair| pair[0].name == pair[1].name)
    {
        return Err(ContractError::config(
            file,
            "stacks.workloads",
            "duplicate_workload",
        ));
    }
    for workload in workloads {
        workload.validate(file)?;
    }
    Ok(())
}
