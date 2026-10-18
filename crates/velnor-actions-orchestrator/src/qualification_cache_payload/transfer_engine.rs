use super::super::manifest::{PayloadManifest, PayloadRoot};
use super::super::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use super::dirs::{
    open_directory_path, path_exists_no_follow, path_points_to, same_version,
    validate_owned_directory,
};
use super::files::digest_file_with_mode;
use super::install::install_roots;
use super::payload;
use super::stage::{verify_staged_payload, write_new_payload};
use super::{CacheExportProof, CacheRestoreProof, CacheRestoreStatus};
use super::{STATE_DOMAIN, digest_bytes, is_file_root, root_name, update_bytes};
use crate::qualification_cache_backend::validate_qualification_cache_backend_observation;
use crate::qualification_cache_runtime::QualificationCacheBindingOutputs;
use std::fs;
use std::path::Path;
use velnor_actions_contract::workflow::{
    BoundQualificationCacheKeys, QualificationCacheBackendObservation,
    QualificationCacheRestoreExpectation,
};

#[path = "transfer_normal_engine.rs"]
pub(crate) mod normal;
pub(crate) use normal::{export_normal_payload, import_normal_payload};

/// Copy checked roots into the stage path derived from the bound save key.
/// The state digest comes from the bytes inventoried through open descriptors.
pub(crate) fn export_payload(
    binding: &QualificationCacheBindingOutputs,
    roots: &CacheRootSet,
    runner_temp: &Path,
) -> Result<CacheExportProof, &'static str> {
    validate_binding_roots(binding, roots)?;
    validate_roots_under_runner_temp(roots, runner_temp)?;
    if !binding.active {
        return Err("qualification_cache_export_disabled");
    }
    let payload_roots = snapshot_roots(roots)?;
    let state_digest = useful_state_digest(&payload_roots)?;
    let keys = bound_keys(binding)?;
    let selected_key = keys
        .save_key_for_state(&state_digest)
        .map_err(|_| "qualification_cache_export_state_invalid")?;
    if selected_key != binding.save_key.as_deref() {
        return Err("qualification_cache_export_key_mismatch");
    }
    let Some(cache_key) = selected_key else {
        return Ok(CacheExportProof {
            state_digest,
            save_key: None,
            save_stage_path: None,
            manifest_digest: None,
        });
    };
    let stage_path = super::super::qualification_cache_stage_path(
        runner_temp,
        &binding.campaign,
        &binding.layer,
        cache_key,
    )?;
    let manifest =
        PayloadManifest::for_save(binding, roots, cache_key, &state_digest, payload_roots)?;
    let manifest_text = manifest.canonical_json()?;
    let manifest_digest = digest_bytes(&manifest_text);
    let created_stage = write_new_payload(&stage_path, runner_temp, roots, &manifest_text)?;
    if let Err(error) = verify_staged_payload(&stage_path, binding, roots, cache_key, false) {
        created_stage.remove()?;
        return Err(error);
    }
    Ok(CacheExportProof {
        state_digest,
        save_key: Some(cache_key.to_owned()),
        save_stage_path: Some(stage_path),
        manifest_digest: Some(manifest_digest),
    })
}

/// Validate backend and staged-payload evidence, then install all roots.
/// A cache miss is recorded without importing any bytes.
pub(crate) fn import_payload(
    binding: &QualificationCacheBindingOutputs,
    roots: &CacheRootSet,
    runner_temp: &Path,
    action_hit: bool,
    action_matched_key: Option<&str>,
    observed: &QualificationCacheBackendObservation,
) -> Result<CacheRestoreProof, &'static str> {
    validate_binding_roots(binding, roots)?;
    validate_roots_under_runner_temp(roots, runner_temp)?;
    let keys = bound_keys(binding)?;
    validate_qualification_cache_backend_observation(
        &keys,
        action_hit,
        action_matched_key,
        observed,
    )
    .map_err(|_| "qualification_cache_restore_backend_mismatch")?;
    if !binding.active {
        return Ok(CacheRestoreProof {
            status: CacheRestoreStatus::Disabled,
            state_digest: None,
            stage_path: None,
        });
    }
    let restore_key = binding
        .restore_key
        .as_deref()
        .ok_or("qualification_cache_restore_key_missing")?;
    let stage_path = super::super::qualification_cache_stage_path(
        runner_temp,
        &binding.campaign,
        &binding.layer,
        restore_key,
    )?;
    if binding.expected_cache.is_none() {
        if path_exists_no_follow(&stage_path)? {
            return Err("qualification_cache_restore_unexpected_stage_on_miss");
        }
        return Ok(CacheRestoreProof {
            status: CacheRestoreStatus::Miss,
            state_digest: None,
            stage_path: None,
        });
    }
    if !action_hit || action_matched_key != Some(restore_key) {
        return Err("qualification_cache_restore_action_mismatch");
    }
    if let QualificationCacheBackendObservation::Found(entry) = observed {
        let matches_action_version =
            super::super::cache_action_version::version_agrees_with_action_path(
                &stage_path,
                &entry.version,
            )?;
        if !matches_action_version {
            return Err("qualification_cache_restore_version_mismatch");
        }
    } else {
        return Err("qualification_cache_restore_backend_mismatch");
    }
    let manifest = verify_staged_payload(&stage_path, binding, roots, restore_key, true)?;
    let state_digest = manifest.state_digest.clone();
    install_roots(
        &stage_path,
        roots,
        runner_temp,
        &manifest.roots,
        &state_digest,
    )?;
    Ok(CacheRestoreProof {
        status: CacheRestoreStatus::Hit,
        state_digest: Some(state_digest),
        stage_path: Some(stage_path),
    })
}

pub(crate) fn validate_binding_roots(
    binding: &QualificationCacheBindingOutputs,
    roots: &CacheRootSet,
) -> Result<(), &'static str> {
    if binding.matrix_key != roots.matrix_key || binding.layer != roots.layer.as_str() {
        return Err("qualification_cache_roots_binding_mismatch");
    }
    Ok(())
}

pub(super) fn validate_roots_under_runner_temp(
    roots: &CacheRootSet,
    runner_temp: &Path,
) -> Result<(), &'static str> {
    validate_clean_absolute(runner_temp)?;
    let root_base = runner_temp.join("velnor");
    let expected: &[CacheRootId] = match roots.layer {
        velnor_actions_contract::workflow::QualificationCacheLayer::MbxObjects => {
            &[CacheRootId::MbxActions]
        }
        velnor_actions_contract::workflow::QualificationCacheLayer::CargoSources => &[
            CacheRootId::CargoGitDb,
            CacheRootId::CargoRegistryCache,
            CacheRootId::CargoRegistryIndex,
        ],
        velnor_actions_contract::workflow::QualificationCacheLayer::MiseTools => &[
            CacheRootId::CargoBin,
            CacheRootId::CargoCrates2Json,
            CacheRootId::CargoCratesToml,
            CacheRootId::MiseData,
            CacheRootId::RustupHome,
        ],
        velnor_actions_contract::workflow::QualificationCacheLayer::TofuProviders => {
            &[CacheRootId::TofuPlugins]
        }
        velnor_actions_contract::workflow::QualificationCacheLayer::TaskResult => {
            &[CacheRootId::TaskArtifacts]
        }
    };
    if roots.roots.len() != expected.len()
        || roots.roots.iter().zip(expected).any(|(root, id)| {
            root.id != *id
                || validate_clean_absolute(&root.path).is_err()
                || !root.path.starts_with(&root_base)
        })
    {
        return Err("qualification_cache_roots_mismatch");
    }
    Ok(())
}

pub(super) fn validate_clean_absolute(path: &Path) -> Result<(), &'static str> {
    let text = path.to_str().ok_or("qualification_cache_path_invalid")?;
    if text.len() > 4_096
        || !text.starts_with('/')
        || text == "/"
        || text.bytes().any(|byte| byte.is_ascii_control())
        || text
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err("qualification_cache_path_invalid");
    }
    Ok(())
}

pub(super) fn bound_keys(
    binding: &QualificationCacheBindingOutputs,
) -> Result<BoundQualificationCacheKeys, &'static str> {
    if binding.expected_cache.is_some() && binding.restore_key.is_none() {
        return Err("qualification_cache_binding_restore_inconsistent");
    }
    let restore =
        binding
            .restore_key
            .as_ref()
            .map(|requested_key| QualificationCacheRestoreExpectation {
                requested_key: requested_key.clone(),
                expected_cache: binding.expected_cache.clone(),
            });
    Ok(BoundQualificationCacheKeys {
        restore,
        save_key: binding.save_key.clone(),
        save_if_state_changes: binding.save_if_state_changes,
        expected_prior_state_digest: binding.expected_prior_state_digest.clone(),
        runtime_identity_digest: binding.runtime_identity_digest.clone(),
    })
}

pub(crate) fn snapshot_roots(roots: &CacheRootSet) -> Result<Vec<PayloadRoot>, &'static str> {
    roots
        .roots
        .iter()
        .map(snapshot_root)
        .collect::<Result<Vec<_>, _>>()
}

pub(crate) fn snapshot_root(root: &CacheRootObservation) -> Result<PayloadRoot, &'static str> {
    let metadata = match fs::symlink_metadata(&root.path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PayloadRoot {
                id: root.id,
                present: false,
                mode: None,
                digest: None,
            });
        }
        Err(_) => return Err("qualification_cache_root_inventory_failed"),
    };
    let observed = if is_file_root(root.id) {
        if !metadata.file_type().is_file() {
            return Err("qualification_cache_root_kind_mismatch");
        }
        digest_file_with_mode(&root.path)?
    } else {
        if !metadata.file_type().is_dir() {
            return Err("qualification_cache_root_kind_mismatch");
        }
        digest_directory(&root.path)?
    };
    let (digest, mode) = observed.map_or((None, None), |(digest, mode)| (Some(digest), Some(mode)));
    Ok(PayloadRoot {
        id: root.id,
        present: digest.is_some(),
        mode,
        digest,
    })
}

pub(super) fn digest_directory(path: &Path) -> Result<Option<(String, u32)>, &'static str> {
    let opened = open_directory_path(path)?;
    validate_owned_directory(&opened)?;
    let before = rustix::fs::fstat(&opened).map_err(|_| "qualification_cache_root_read_failed")?;
    let digest = payload::digest_stage(path)?;
    let after = rustix::fs::fstat(&opened).map_err(|_| "qualification_cache_root_read_failed")?;
    if !same_version(&before, &after) || !path_points_to(path, &opened) {
        return Err("qualification_cache_root_changed");
    }
    Ok(digest.map(|digest| (digest, u32::from(before.st_mode) & 0o777)))
}

pub(crate) fn useful_state_digest(roots: &[PayloadRoot]) -> Result<String, &'static str> {
    if roots.is_empty()
        || roots
            .windows(2)
            .any(|pair| root_name(pair[0].id) >= root_name(pair[1].id))
        || roots
            .iter()
            .any(|root| root.present != root.digest.is_some())
    {
        return Err("qualification_cache_roots_mismatch");
    }
    let mut digest = blake3::Hasher::new();
    digest.update(STATE_DOMAIN);
    digest.update(
        &velnor_actions_contract::workflow::QUALIFICATION_CACHE_FORMAT_VERSION.to_be_bytes(),
    );
    for root in roots {
        update_bytes(&mut digest, root_name(root.id).as_bytes());
        if root.present {
            digest.update(b"present\0");
            let mode = root
                .mode
                .ok_or("qualification_cache_payload_root_mode_missing")?;
            digest.update(&mode.to_be_bytes());
            let value = root
                .digest
                .as_deref()
                .ok_or("qualification_cache_payload_digest_missing")?;
            update_bytes(&mut digest, value.as_bytes());
        } else {
            if root.mode.is_some() {
                return Err("qualification_cache_roots_mismatch");
            }
            digest.update(b"absent\0");
        }
    }
    Ok(format!("b3-{}", digest.finalize().to_hex()))
}
