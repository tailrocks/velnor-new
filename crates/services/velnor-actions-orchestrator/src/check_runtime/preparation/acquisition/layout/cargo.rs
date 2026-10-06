//! Verified Cargo archives become one owned offline source/vendor closure.
use crate::OrchestratorError;
use crate::internal::internal;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use velnor_actions_contract_config::config::{
    CheckPlatform, QualifiedCargoInstallation, QualifiedTool, QualifiedToolBackend,
    QualifiedToolOptions, QualifiedToolPlatform,
};
use velnor_actions_mise::CheckDeadline;

mod custody;
mod lock;

pub(super) fn prepare_cargo_source(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    tool_home: &Path,
    primary_roots: &[PathBuf],
    dependency_roots: &[PathBuf],
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    let (crate_name, lock_sha, qualification) = admission(tool, platform)?;
    if primary_roots.len() != 1
        || qualification.artifacts.len() != 1
        || dependency_roots.len() != qualification.dependency_artifacts.len()
    {
        return Err(internal("qualified_cargo_archive_count"));
    }
    custody::reject_ancestor_configs(tool_home)?;
    require_cargo_home(tool_home)?;
    let primary = custody::crate_root(&primary_roots[0])?;
    custody::verify_owned_root(tool_home, &primary)?;
    custody::inspect_tree(&primary, deadline)?;
    let identity = lock::package_identity(&primary, deadline)?;
    if identity != (crate_name.to_owned(), tool.version.clone())
        || lock::archive_identity(&qualification.artifacts[0].url)? != identity
    {
        return Err(internal("qualified_cargo_primary_identity"));
    }
    lock::verify_closure(&primary, lock_sha, qualification, &identity, deadline)?;
    let mut dependencies = Vec::new();
    let mut identities = BTreeSet::new();
    for (extracted, artifact) in dependency_roots
        .iter()
        .zip(&qualification.dependency_artifacts)
    {
        check_deadline(deadline)?;
        let root = custody::crate_root(extracted)?;
        custody::verify_owned_root(tool_home, &root)?;
        custody::inspect_tree(&root, deadline)?;
        let identity = lock::package_identity(&root, deadline)?;
        if identity != lock::archive_identity(&artifact.url)?
            || !identities.insert(identity.clone())
        {
            return Err(internal("qualified_cargo_dependency_identity"));
        }
        dependencies.push((root, identity, &artifact.sha256));
    }
    let source = tool_home.join("sources");
    let vendor = tool_home.join("vendor");
    custody::require_absent(&source)?;
    custody::require_absent(&vendor)?;
    fs::create_dir(&vendor)
        .map_err(|error| OrchestratorError::io(vendor.display().to_string(), error.to_string()))?;
    fs::rename(&primary, &source)
        .map_err(|error| OrchestratorError::io(source.display().to_string(), error.to_string()))?;
    for (root, identity, checksum) in dependencies {
        check_deadline(deadline)?;
        let destination = vendor.join(format!("{}-{}", identity.0, identity.1));
        custody::require_absent(&destination)?;
        fs::rename(root, &destination).map_err(|error| {
            OrchestratorError::io(destination.display().to_string(), error.to_string())
        })?;
        custody::write_checksum(&destination, checksum, deadline)?;
    }
    custody::reject_ancestor_configs(&source)?;
    check_deadline(deadline)?;
    write_source_config(tool_home, &vendor)
}

fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}

fn admission(
    tool: &QualifiedTool,
    platform: CheckPlatform,
) -> Result<(&str, &str, &QualifiedToolPlatform), OrchestratorError> {
    tool.validate("qualified_tools", &tool.id)?;
    let QualifiedToolBackend::Cargo { crate_name } = &tool.backend else {
        return Err(internal("qualified_cargo_backend"));
    };
    let QualifiedToolOptions::Cargo {
        installation: QualifiedCargoInstallation::Source { source_lock_sha256 },
        ..
    } = &tool.options
    else {
        return Err(internal("qualified_cargo_source_mode"));
    };
    let qualification = tool
        .platforms
        .iter()
        .find(|entry| entry.platform == platform)
        .ok_or_else(|| internal("qualified_cargo_platform"))?;
    Ok((crate_name, source_lock_sha256, qualification))
}

fn write_source_config(tool_home: &Path, vendor: &Path) -> Result<(), OrchestratorError> {
    let directory = vendor
        .to_str()
        .ok_or_else(|| internal("qualified_cargo_non_utf8"))?;
    let text = format!(
        "[source.crates-io]\nreplace-with = \"velnor-vendor\"\n\n[source.velnor-vendor]\ndirectory = {}\n",
        toml::Value::String(directory.to_owned())
    );
    crate::exclusive_write::write_exclusive(
        &tool_home.join("cargo-source.toml"),
        text.as_bytes(),
        "qualified_cargo_config",
    )
}

fn require_cargo_home(tool_home: &Path) -> Result<(), OrchestratorError> {
    let cargo = tool_home.join("cargo");
    let metadata = fs::symlink_metadata(&cargo)
        .map_err(|error| OrchestratorError::io(cargo.display().to_string(), error.to_string()))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || fs::read_dir(&cargo)
            .map_err(|error| OrchestratorError::io(cargo.display().to_string(), error.to_string()))?
            .next()
            .is_some()
    {
        return Err(internal("qualified_cargo_home_not_empty_owned_directory"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
