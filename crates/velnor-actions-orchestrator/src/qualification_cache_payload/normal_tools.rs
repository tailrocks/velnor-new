//! Normal Mise tools cache payload operations.

use std::path::Path;

use super::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use super::transfer::engine;
use super::{
    CacheActionObservation, CacheExportProof, CacheRestoreProof, normal_tools_cache_stage_path,
};
use velnor_actions_contract::QualificationCacheLayer;

/// Inputs for the normal Mise tools payload, read from the checked runtime action.
#[derive(Debug, Clone, Copy)]
pub struct NormalToolsCachePayloadRequest<'a> {
    /// Runner temporary root from the current job.
    pub runner_temp: &'a Path,
    /// Current user home directory.
    pub home: &'a Path,
    /// Runtime identity emitted by the trusted post-setup action.
    pub runtime_identity: &'a str,
    /// Full key emitted by the normal tools cache identity step.
    pub cache_key: &'a str,
    /// Mise data directory emitted by the trusted runtime action.
    pub mise_data_dir: &'a Path,
    /// Cargo home configured for the job.
    pub cargo_home: &'a Path,
    /// Rustup home configured for the job.
    pub rustup_home: &'a Path,
}

/// Record the normal tools payload under its full cache key.
///
/// # Errors
///
/// Gives a static error code when the runtime identity, cache key, roots, or
/// payload staging fails validation.
pub fn export_normal_tools_cache_payload(
    request: &NormalToolsCachePayloadRequest<'_>,
) -> Result<CacheExportProof, &'static str> {
    let roots = normal_tools_roots(request)?;
    engine::export_normal_payload(
        request.runtime_identity,
        request.cache_key,
        &roots,
        request.runner_temp,
        request.home,
        &normal_tools_cache_stage_path(request.runner_temp, request.cache_key)?,
    )
}

/// Check action output and the v3 manifest before restoring normal tools.
///
/// # Errors
///
/// Gives a static error code when the action output, manifest, staged bytes, or
/// live roots fail validation.
pub fn import_normal_tools_cache_payload(
    request: &NormalToolsCachePayloadRequest<'_>,
    action: &CacheActionObservation,
) -> Result<CacheRestoreProof, &'static str> {
    validate_action_observation(action)?;
    let roots = normal_tools_roots(request)?;
    engine::import_normal_payload(request, &roots, action.hit, action.matched_key.as_deref())
}

fn normal_tools_roots(
    request: &NormalToolsCachePayloadRequest<'_>,
) -> Result<CacheRootSet, &'static str> {
    clean_path_text(request.runner_temp, "normal_cache_binding_invalid")?;
    clean_path_text(request.home, "normal_cache_binding_invalid")?;
    let expected_cargo_home = request.runner_temp.join("velnor/cargo");
    let expected_rustup_home = request.runner_temp.join("velnor/rustup");
    let expected_mise_data = request.home.join(".local/share/mise");
    if request.cargo_home != expected_cargo_home
        || request.rustup_home != expected_rustup_home
        || request.mise_data_dir != expected_mise_data
    {
        return Err("normal_cache_roots_mismatch");
    }
    Ok(CacheRootSet {
        matrix_key: "normal-tools".to_owned(),
        layer: QualificationCacheLayer::MiseTools,
        roots: vec![
            CacheRootObservation {
                id: CacheRootId::CargoBin,
                path: request.cargo_home.join("bin"),
            },
            CacheRootObservation {
                id: CacheRootId::CargoCrates2Json,
                path: request.cargo_home.join(".crates2.json"),
            },
            CacheRootObservation {
                id: CacheRootId::CargoCratesToml,
                path: request.cargo_home.join(".crates.toml"),
            },
            CacheRootObservation {
                id: CacheRootId::MiseData,
                path: request.mise_data_dir.to_owned(),
            },
            CacheRootObservation {
                id: CacheRootId::RustupHome,
                path: request.rustup_home.to_owned(),
            },
        ],
    })
}

fn validate_action_observation(action: &CacheActionObservation) -> Result<(), &'static str> {
    if action.matched_key.as_ref().is_some_and(|key| {
        key.is_empty()
            || key.len() > velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
            || !key.bytes().all(|byte| byte.is_ascii_graphic())
    }) || (action.hit != action.matched_key.is_some())
    {
        return Err("qualification_cache_action_observation_invalid");
    }
    Ok(())
}

fn clean_path_text<'a>(path: &'a Path, error: &'static str) -> Result<&'a str, &'static str> {
    let text = path.to_str().ok_or(error)?;
    if text.len() > 4_096
        || !text.starts_with('/')
        || text == "/"
        || text.bytes().any(|byte| byte.is_ascii_control())
        || text
            .split('/')
            .skip(1)
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(error);
    }
    Ok(text)
}
