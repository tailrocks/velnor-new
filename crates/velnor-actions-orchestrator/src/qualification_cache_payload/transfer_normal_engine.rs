use super::super::super::NormalToolsCachePayloadRequest;
use super::super::super::manifest::PayloadManifest;
use super::super::super::roots::{CacheRootId, CacheRootSet};
use super::super::dirs::path_exists_no_follow;
use super::super::install::install_normal_tools_roots;
use super::super::stage::{
    remove_untrusted_stage, verify_staged_normal_payload, write_new_payload,
};
use super::super::{CacheExportProof, CacheRestoreProof, CacheRestoreStatus};
use super::{digest_bytes, snapshot_roots, useful_state_digest, validate_clean_absolute};
use std::path::Path;
use velnor_actions_contract::QualificationCacheLayer;

pub(crate) fn normal_identity_digest(identity: &str) -> Result<String, &'static str> {
    if !valid_identity(identity) {
        return Err("normal_cache_identity_invalid");
    }
    Ok(digest_bytes(identity))
}

pub(crate) fn export_normal_payload(
    identity: &str,
    cache_key: &str,
    roots: &CacheRootSet,
    runner_temp: &Path,
    home: &Path,
    stage_path: &Path,
) -> Result<CacheExportProof, &'static str> {
    let identity_digest = normal_identity_digest(identity)?;
    validate_normal_binding(identity, cache_key, roots, runner_temp, home, stage_path)?;
    if path_exists_no_follow(stage_path)? {
        remove_untrusted_stage(stage_path, runner_temp)?;
    }
    let payload_roots = snapshot_roots(roots)?;
    let state_digest = useful_state_digest(&payload_roots)?;
    let manifest = PayloadManifest::for_normal_save(
        cache_key,
        &identity_digest,
        &state_digest,
        roots,
        payload_roots,
    )?;
    let manifest_text = manifest.canonical_json()?;
    let manifest_digest = digest_bytes(&manifest_text);
    let created_stage = write_new_payload(stage_path, runner_temp, roots, &manifest_text)?;
    if let Err(error) = verify_staged_normal_payload(stage_path, roots, cache_key, &identity_digest)
    {
        created_stage.remove()?;
        return Err(error);
    }
    Ok(CacheExportProof {
        state_digest,
        save_key: Some(cache_key.to_owned()),
        save_stage_path: Some(stage_path.to_owned()),
        manifest_digest: Some(manifest_digest),
    })
}

pub(crate) fn import_normal_payload(
    request: &NormalToolsCachePayloadRequest<'_>,
    roots: &CacheRootSet,
    action_hit: bool,
    action_matched_key: Option<&str>,
) -> Result<CacheRestoreProof, &'static str> {
    let identity = request.runtime_identity;
    let cache_key = request.cache_key;
    let runner_temp = request.runner_temp;
    let home = request.home;
    let stage_path = super::super::super::normal_tools_cache_stage_path(runner_temp, cache_key)?;
    let identity_digest = normal_identity_digest(identity)?;
    validate_normal_binding(identity, cache_key, roots, runner_temp, home, &stage_path)?;
    if !action_hit {
        if action_matched_key.is_some() {
            return Err("normal_cache_restore_action_mismatch");
        }
        if path_exists_no_follow(&stage_path)? {
            return Err("normal_cache_restore_unexpected_stage_on_miss");
        }
        return Ok(CacheRestoreProof {
            status: CacheRestoreStatus::Miss,
            state_digest: None,
            stage_path: None,
        });
    }
    if action_matched_key != Some(cache_key) {
        let error = cleanup_stage_after_error(
            &stage_path,
            runner_temp,
            "normal_cache_restore_action_mismatch",
        )?;
        return Err(error);
    }
    let manifest =
        match verify_staged_normal_payload(&stage_path, roots, cache_key, &identity_digest) {
            Ok(manifest) => manifest,
            Err(error) => {
                let error = cleanup_stage_after_error(&stage_path, runner_temp, error)?;
                return Err(error);
            }
        };
    let state_digest = manifest.state_digest.clone();
    if let Err(error) = install_normal_tools_roots(
        &stage_path,
        roots,
        runner_temp,
        home,
        &manifest.roots,
        &state_digest,
    ) {
        let error = cleanup_stage_after_error(&stage_path, runner_temp, error)?;
        return Err(error);
    }
    Ok(CacheRestoreProof {
        status: CacheRestoreStatus::Hit,
        state_digest: Some(state_digest),
        stage_path: Some(stage_path),
    })
}

fn cleanup_stage_after_error(
    stage_path: &Path,
    runner_temp: &Path,
    original_error: &'static str,
) -> Result<&'static str, &'static str> {
    if path_exists_no_follow(stage_path)? {
        remove_untrusted_stage(stage_path, runner_temp)
            .map_err(|_| "normal_cache_restore_stage_cleanup_failed")?;
    }
    Ok(original_error)
}

fn validate_normal_binding(
    identity: &str,
    cache_key: &str,
    roots: &CacheRootSet,
    runner_temp: &Path,
    home: &Path,
    stage_path: &Path,
) -> Result<(), &'static str> {
    if !valid_identity(identity)
        || cache_key != format!("mise-tools-v2-payload-v3-{identity}")
        || roots.layer != QualificationCacheLayer::MiseTools
        || roots.owner_id != "normal-tools"
        || roots.roots.iter().map(|root| root.id).collect::<Vec<_>>()
            != [
                CacheRootId::CargoBin,
                CacheRootId::CargoCrates2Json,
                CacheRootId::CargoCratesToml,
                CacheRootId::MiseData,
                CacheRootId::RustupHome,
            ]
        || validate_clean_absolute(runner_temp).is_err()
        || validate_clean_absolute(home).is_err()
    {
        return Err("normal_cache_binding_invalid");
    }
    let cargo = runner_temp.join("velnor/cargo");
    let expected = [
        (CacheRootId::CargoBin, cargo.join("bin")),
        (CacheRootId::CargoCrates2Json, cargo.join(".crates2.json")),
        (CacheRootId::CargoCratesToml, cargo.join(".crates.toml")),
        (CacheRootId::MiseData, home.join(".local/share/mise")),
        (CacheRootId::RustupHome, runner_temp.join("velnor/rustup")),
    ];
    if roots
        .roots
        .iter()
        .zip(expected)
        .any(|(root, (id, path))| root.id != id || root.path != path)
    {
        return Err("normal_cache_roots_mismatch");
    }
    let expected_stage =
        super::super::super::normal_tools_cache_stage_path(runner_temp, cache_key)?;
    if stage_path != expected_stage {
        return Err("normal_cache_stage_path_mismatch");
    }
    Ok(())
}

fn valid_identity(identity: &str) -> bool {
    identity.len() == 64
        && identity
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
