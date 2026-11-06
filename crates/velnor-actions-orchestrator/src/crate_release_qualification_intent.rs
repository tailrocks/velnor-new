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
    bind_config_path(inputs, metadata.metadata_config())?;
    bind_historical_manifest(metadata.metadata_config(), manifest)?;
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
    crate::release::release_emit::derive_intent_from_snapshot(
        source,
        &config,
        &mapped,
        manifest.bytes(),
        metadata.metadata_config(),
        repository,
        default_branch,
    )
}

pub(crate) fn bind_historical_manifest(
    metadata: &velnor_actions_contract::config::ReleaseRehearsalMetadataConfig,
    manifest: &HistoricalManifest,
) -> Result<(), OrchestratorError> {
    bind_manifest_values(
        &metadata.qualification.historical_manifest,
        manifest.bytes(),
        manifest.source_sha(),
    )
}

fn bind_manifest_values(
    policy: &velnor_actions_contract::config::ReleaseHistoricalManifestPolicy,
    bytes: &[u8],
    source: &str,
) -> Result<(), OrchestratorError> {
    if source != policy.source_sha || crate::planner::generator::sha256_hex(bytes) != policy.sha256
    {
        return Err(invalid());
    }
    let raw = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let parsed =
        velnor_actions_workflow_renderer::release_helpers::validate_consumer_manifest_for_version(
            velnor_actions_contract::ReleaseTarget::LinuxX86_64,
            &policy.version,
            Some(raw),
        )
        .map_err(|_| invalid())?;
    if parsed.commit != policy.source_sha {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn bind_config_path(
    inputs: &CheckoutInputs,
    metadata: &velnor_actions_contract::config::ReleaseRehearsalMetadataConfig,
) -> Result<(), OrchestratorError> {
    if inputs.config_path() != metadata.intent_config_path {
        return Err(invalid());
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_historical_pin_rejects_manifest_and_identity_substitution() {
        let bytes = include_bytes!("../tests/attestation_fixtures/release-manifest.json");
        let mut policy = velnor_actions_contract::config::ReleaseHistoricalManifestPolicy {
            version: "0.1.5".into(),
            source_sha: "f1041f322c54cd7edfdef06afd307799eeaa801a".into(),
            sha256: "e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52".into(),
        };
        assert!(bind_manifest_values(&policy, bytes, &policy.source_sha).is_ok());
        let mut changed = bytes.to_vec();
        changed.push(b' ');
        assert!(bind_manifest_values(&policy, &changed, &policy.source_sha).is_err());
        assert!(bind_manifest_values(&policy, bytes, &"0".repeat(40)).is_err());
        policy.version = "0.1.6".into();
        assert!(bind_manifest_values(&policy, bytes, &policy.source_sha).is_err());
        policy.version = "0.1.5".into();
        policy.sha256 = "0".repeat(64);
        assert!(bind_manifest_values(&policy, bytes, &policy.source_sha).is_err());
    }
}
