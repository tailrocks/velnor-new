//! Bounded, immutable projection of a declared `OrbStack` SDK bundle.

use crate::OrchestratorError;
use crate::internal::internal;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use velnor_actions_contract_config::config::HostOrbStackSdk;
use velnor_actions_mise::CheckDeadline;

mod tree;

/// Retained, independently observed SDK identity used by the container gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SdkProjection {
    pub(crate) orbctl_program: PathBuf,
    pub(crate) app_bundle: PathBuf,
    pub(crate) source_bundle: PathBuf,
    pub(crate) owned_bundle: PathBuf,
    pub(crate) declared_info_plist_sha256: String,
    pub(crate) observed_info_plist_sha256: String,
    pub(crate) declared_main_executable_sha256: String,
    pub(crate) observed_main_executable_sha256: String,
    pub(crate) declared_source_tree_sha256: String,
    pub(crate) observed_source_tree_sha256: String,
    pub(crate) declared_owned_tree_sha256: String,
    pub(crate) observed_owned_tree_sha256: String,
    pub(crate) declared_cli_sha256: String,
    pub(crate) observed_cli_sha256: String,
    pub(crate) cli_sha256: String,
}

struct SourceIdentity {
    source: PathBuf,
    entries: Vec<tree::Entry>,
    observed_info: String,
    observed_main: String,
    observed_source: String,
}

/// Verify the outer app, retain the complete nested CLI bundle, and freeze it.
#[cfg(test)]
pub(crate) fn project_sdk(
    home: &Path,
    sdk: &HostOrbStackSdk,
) -> Result<SdkProjection, OrchestratorError> {
    project_sdk_until(home, sdk, None)
}

pub(crate) fn project_sdk_until(
    home: &Path,
    sdk: &HostOrbStackSdk,
    deadline: Option<CheckDeadline>,
) -> Result<SdkProjection, OrchestratorError> {
    checkpoint(deadline)?;
    let app = existing_directory(Path::new(&sdk.app_bundle_path), "sdk_app_bundle")?;
    let home = existing_directory(home, "sdk_home")?;
    let SourceIdentity {
        source,
        entries,
        observed_info,
        observed_main,
        observed_source,
    } = verify_source(&app, sdk, deadline)?;
    let rel_cli = relative(&sdk.cli_relative_path, "sdk_cli_path")?;
    let projection_root = home.join("orbstack-sdk");
    let name = source
        .file_name()
        .ok_or_else(|| unsafe_path(&source, "sdk_bundle_name"))?;
    let destination = projection_root.join(name);
    if projection_root.starts_with(&app) || projection_root.starts_with(&source) {
        return Err(unsafe_path(
            &projection_root,
            "sdk_destination_inside_source",
        ));
    }
    tree::create_destination(&projection_root)?;
    if let Err(error) = tree::create_destination(&destination) {
        return abort(&projection_root, error);
    }
    if let Err(error) = tree::populate_until(&destination, &entries, deadline) {
        return abort(&projection_root, error);
    }
    if let Err(error) = tree::set_directory_mode(&projection_root, 0o700) {
        return abort(&projection_root, error);
    }
    let owned_entries = match tree::collect_tree_until(&destination, deadline) {
        Ok(entries) => entries,
        Err(error) => return abort(&projection_root, error),
    };
    if let Err(error) = tree::verify_owned_root(&projection_root) {
        return abort(&projection_root, error);
    }
    if let Err(error) = tree::verify_owned_root(&destination) {
        return abort(&projection_root, error);
    }
    let observed_owned = match tree::tree_digest_until(&owned_entries, deadline) {
        Ok(hash) => hash,
        Err(error) => return abort(&projection_root, error),
    };
    if observed_owned != sdk.owned_tree_sha256 {
        return abort(&projection_root, internal("sdk_owned_tree_sha256"));
    }
    let owned_cli = destination.join(&rel_cli);
    let owned_cli_hash = match checked_hash_until(
        &owned_cli,
        &sdk.cli_sha256,
        "sdk_owned_cli_sha256",
        deadline,
    ) {
        Ok(hash) => hash,
        Err(error) => return abort(&projection_root, error),
    };
    Ok(SdkProjection {
        orbctl_program: owned_cli,
        app_bundle: app,
        source_bundle: source,
        owned_bundle: destination,
        declared_info_plist_sha256: sdk.info_plist_sha256.clone(),
        observed_info_plist_sha256: observed_info,
        declared_main_executable_sha256: sdk.main_executable_sha256.clone(),
        observed_main_executable_sha256: observed_main,
        declared_source_tree_sha256: sdk.source_tree_sha256.clone(),
        observed_source_tree_sha256: observed_source,
        declared_owned_tree_sha256: sdk.owned_tree_sha256.clone(),
        observed_owned_tree_sha256: observed_owned,
        declared_cli_sha256: sdk.cli_sha256.clone(),
        observed_cli_sha256: owned_cli_hash.clone(),
        cli_sha256: owned_cli_hash,
    })
}

fn verify_source(
    app: &Path,
    sdk: &HostOrbStackSdk,
    deadline: Option<CheckDeadline>,
) -> Result<SourceIdentity, OrchestratorError> {
    checkpoint(deadline)?;
    let info = app.join("Contents/Info.plist");
    reject_links(&info)?;
    let observed_info = checked_hash_until(
        &info,
        &sdk.info_plist_sha256,
        "sdk_info_plist_sha256",
        deadline,
    )?;
    let main = app.join(relative(&sdk.main_executable_path, "sdk_main_path")?);
    reject_links(&main)?;
    let observed_main = checked_hash_until(
        &main,
        &sdk.main_executable_sha256,
        "sdk_main_executable_sha256",
        deadline,
    )?;
    let source = declared_bundle(app, &sdk.cli_bundle_path)?;
    let rel_cli = relative(&sdk.cli_relative_path, "sdk_cli_path")?;
    let entries = tree::collect_tree_until(&source, deadline)?;
    let observed_source = tree::tree_digest_until(&entries, deadline)?;
    require(
        &observed_source,
        &sdk.source_tree_sha256,
        "sdk_source_tree_sha256",
    )?;
    checked_hash_until(
        &source.join(rel_cli),
        &sdk.cli_sha256,
        "sdk_cli_sha256",
        deadline,
    )?;
    checkpoint(deadline)?;
    Ok(SourceIdentity {
        source,
        entries,
        observed_info,
        observed_main,
        observed_source,
    })
}

/// Recheck source identity and the retained tree around each readonly probe.
#[cfg(test)]
pub(crate) fn revalidate_sdk(
    projection: &SdkProjection,
    sdk: &HostOrbStackSdk,
) -> Result<(), OrchestratorError> {
    revalidate_sdk_until(projection, sdk, None)
}

pub(crate) fn revalidate_sdk_until(
    projection: &SdkProjection,
    sdk: &HostOrbStackSdk,
    deadline: Option<CheckDeadline>,
) -> Result<(), OrchestratorError> {
    checkpoint(deadline)?;
    let app = existing_directory(&projection.app_bundle, "sdk_app_bundle")?;
    let info_path = app.join("Contents/Info.plist");
    reject_links(&info_path)?;
    let info = checked_hash_until(
        &info_path,
        &sdk.info_plist_sha256,
        "sdk_info_plist_sha256",
        deadline,
    )?;
    if info != projection.observed_info_plist_sha256 {
        return Err(internal("sdk_info_plist_changed"));
    }
    let main_rel = relative(&sdk.main_executable_path, "sdk_main_path")?;
    let main_path = app.join(main_rel);
    reject_links(&main_path)?;
    let main = checked_hash_until(
        &main_path,
        &sdk.main_executable_sha256,
        "sdk_main_executable_sha256",
        deadline,
    )?;
    if main != projection.observed_main_executable_sha256 {
        return Err(internal("sdk_main_executable_changed"));
    }
    let source = sdk_tree_sha256_until(&projection.source_bundle, deadline)?;
    if source != sdk.source_tree_sha256 || source != projection.observed_source_tree_sha256 {
        return Err(internal("sdk_source_tree_changed"));
    }
    let root = projection
        .owned_bundle
        .parent()
        .ok_or_else(|| internal("sdk_owned_root"))?;
    existing_directory(root, "sdk_owned_root")?;
    tree::verify_owned_root(root)?;
    tree::verify_owned_root(&projection.owned_bundle)?;
    let owned = sdk_tree_sha256_until(&projection.owned_bundle, deadline)?;
    if owned != sdk.owned_tree_sha256 || owned != projection.observed_owned_tree_sha256 {
        return Err(internal("sdk_owned_tree_changed"));
    }
    let rel_cli = relative(&sdk.cli_relative_path, "sdk_cli_path")?;
    let cli = checked_hash_until(
        &projection.owned_bundle.join(rel_cli),
        &sdk.cli_sha256,
        "sdk_owned_cli_sha256",
        deadline,
    )?;
    if cli != projection.observed_cli_sha256 {
        return Err(internal("sdk_cli_changed"));
    }
    checkpoint(deadline)
}

/// Compute the SDK tree identity with the projection's canonical recipe.
pub(crate) fn sdk_tree_sha256_until(
    root: &Path,
    deadline: Option<CheckDeadline>,
) -> Result<String, OrchestratorError> {
    let entries = tree::collect_tree_until(root, deadline)?;
    tree::tree_digest_until(&entries, deadline)
}

fn existing_directory(path: &Path, reason: &str) -> Result<PathBuf, OrchestratorError> {
    if !path.is_absolute() {
        return Err(unsafe_path(path, reason));
    }
    reject_links(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(unsafe_path(path, "sdk_not_directory"));
    }
    let canonical = path.canonicalize().map_err(|error| io_error(path, error))?;
    if canonical != path {
        return Err(unsafe_path(path, "sdk_noncanonical_path"));
    }
    Ok(canonical)
}

fn declared_bundle(app: &Path, raw: &str) -> Result<PathBuf, OrchestratorError> {
    let path = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        app.join(relative(raw, "sdk_bundle_path")?)
    };
    let bundle = existing_directory(&path, "sdk_cli_bundle")?;
    if bundle == app || !bundle.starts_with(app) {
        return Err(unsafe_path(&bundle, "sdk_cli_bundle_escape"));
    }
    Ok(bundle)
}

fn relative(raw: &str, reason: &str) -> Result<PathBuf, OrchestratorError> {
    let path = Path::new(raw);
    if raw.is_empty() || path.is_absolute() || raw.contains('\0') || raw.contains('\\') {
        return Err(unsafe_path(path, reason));
    }
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(unsafe_path(path, reason));
        };
        if part.to_str().is_none() {
            return Err(unsafe_path(path, "sdk_non_utf8_path"));
        }
    }
    Ok(path.to_path_buf())
}

fn reject_links(path: &Path) -> Result<(), OrchestratorError> {
    if !path.is_absolute() {
        return Err(unsafe_path(path, "sdk_absolute_path"));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push("/"),
            Component::Normal(part) => {
                current.push(part);
                let metadata =
                    fs::symlink_metadata(&current).map_err(|error| io_error(&current, error))?;
                if metadata.file_type().is_symlink() {
                    return Err(unsafe_path(&current, "sdk_symlink_rejected"));
                }
            }
            _ => return Err(unsafe_path(path, "sdk_path_component")),
        }
    }
    Ok(())
}

fn checked_hash_until(
    path: &Path,
    expected: &str,
    reason: &str,
    deadline: Option<CheckDeadline>,
) -> Result<String, OrchestratorError> {
    let observed = tree::file_hash_until(path, deadline)?;
    require(&observed, expected, reason)
}

fn checkpoint(deadline: Option<CheckDeadline>) -> Result<(), OrchestratorError> {
    if let Some(deadline) = deadline {
        deadline
            .remaining()
            .map_err(|error| crate::internal::internal(&error.to_string()))?;
    }
    Ok(())
}

fn require(observed: &str, expected: &str, reason: &str) -> Result<String, OrchestratorError> {
    if observed != expected {
        return Err(internal(reason));
    }
    Ok(observed.to_owned())
}

fn abort<T>(destination: &Path, error: OrchestratorError) -> Result<T, OrchestratorError> {
    if fs::remove_dir_all(destination).is_err() {
        return Err(internal("sdk_projection_cleanup"));
    }
    Err(error)
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}

#[cfg(test)]
mod tests;
