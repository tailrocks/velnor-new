// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_qualification_bundle::MetadataEvidence;
use crate::crate_release_qualification_inputs::CheckoutInputs;
use crate::crate_release_qualification_manifest::HistoricalManifest;
use crate::crate_release_qualification_snapshot::SourceSnapshot;
use crate::{OrchestratorError, internal::internal};
use serde_json::Value;
use std::path::{Component, Path};
use velnor_actions_contract::config::CrateReleaseIntent;

pub fn derive_intent_from_evidence(
    source: &SourceSnapshot,
    inputs: &CheckoutInputs,
    metadata: &MetadataEvidence,
    manifest: &HistoricalManifest,
    repository: &str,
    default_branch: &str,
) -> Result<CrateReleaseIntent, OrchestratorError> {
    if source.source_sha() != metadata.source_sha()
        || source.tree_sha() != metadata.source_tree_sha()
        || source.source_sha() != inputs.source_sha()
        || !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || !velnor_actions_contract::is_valid_branch_name(default_branch)
    {
        return Err(invalid());
    }
    let config = crate::config::load_config_from_text(inputs.config())?;
    if config
        .workflow
        .default_branch
        .as_deref()
        .is_some_and(|branch| branch != default_branch)
    {
        return Err(invalid());
    }
    let mapped = remap_metadata(metadata.bytes(), source.root())?;
    crate::release_emit::derive_intent_from_snapshot(
        source,
        &config,
        &mapped,
        manifest.bytes(),
        metadata.metadata_config(),
        repository,
        default_branch,
    )
}

pub(crate) fn remap_metadata(bytes: &[u8], root: &Path) -> Result<String, OrchestratorError> {
    let mut value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(bytes, 16 * 1024 * 1024)
            .map_err(|_| invalid())?;
    remap_field(&mut value, "workspace_root", root)?;
    let packages = value
        .get_mut("packages")
        .and_then(Value::as_array_mut)
        .ok_or_else(invalid)?;
    if packages.is_empty() || packages.len() > 10000 {
        return Err(invalid());
    }
    for package in packages {
        remap_field(package, "manifest_path", root)?;
        for target in package
            .get_mut("targets")
            .and_then(Value::as_array_mut)
            .ok_or_else(invalid)?
        {
            remap_field(target, "src_path", root)?;
        }
        for dependency in package
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .ok_or_else(invalid)?
        {
            if dependency.get("path").is_some_and(|path| !path.is_null()) {
                remap_field(dependency, "path", root)?;
            }
        }
    }
    serde_json::to_string(&value).map_err(|_| invalid())
}

fn remap_field(value: &mut Value, field: &str, root: &Path) -> Result<(), OrchestratorError> {
    let item = value.get_mut(field).ok_or_else(invalid)?;
    let path = Path::new(item.as_str().ok_or_else(invalid)?);
    let relative = path.strip_prefix("/source").map_err(|_| invalid())?;
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(invalid());
    }
    let mapped = root.join(relative);
    *item = Value::String(mapped.to_str().ok_or_else(invalid)?.to_owned());
    Ok(())
}

fn invalid() -> OrchestratorError {
    internal("qualification_intent_evidence_invalid")
}
