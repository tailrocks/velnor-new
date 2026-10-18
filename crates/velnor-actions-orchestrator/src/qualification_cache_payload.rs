//! Checked cache payload operations for generated consumer jobs.

use std::env;
use std::path::{Path, PathBuf};

use velnor_actions_contract::QualificationCacheLayer;
use velnor_actions_contract::workflow::QualificationCacheBackendObservation;

use crate::qualification_cache_runtime::{
    QualificationCacheBindingOutputs, QualificationCacheBindingRequest,
    bind_qualification_cache_runtime,
};

#[path = "qualification_cache_payload/manifest.rs"]
mod manifest;
#[path = "qualification_cache_payload/roots.rs"]
mod roots;
#[path = "qualification_cache_payload/stage_paths.rs"]
mod stage_paths;
pub use stage_paths::{normal_tools_cache_stage_path, qualification_cache_stage_path};
#[path = "qualification_cache_payload/transfer.rs"]
mod transfer;

/// Actual configured cache paths from the generated job environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct QualificationCacheRootEnvironment<'a> {
    /// Mise's configured cache directory for package actions.
    pub mbx_cache_dir: Option<&'a Path>,
    /// Cargo home from the job environment.
    pub cargo_home: Option<&'a Path>,
    /// Mise's configured Cargo home.
    pub mise_cargo_home: Option<&'a Path>,
    /// Mise's configured data directory.
    pub mise_data_dir: Option<&'a Path>,
    /// Rustup home from the job environment.
    pub rustup_home: Option<&'a Path>,
    /// Mise's configured Rustup home.
    pub mise_rustup_home: Option<&'a Path>,
    /// `OpenTofu` provider cache directory.
    pub tofu_plugin_cache_dir: Option<&'a Path>,
    /// Mise task result cache directory.
    pub mise_task_cache_dir: Option<&'a Path>,
}

impl QualificationCacheRootEnvironment<'_> {
    fn value(self, name: &str) -> Option<String> {
        let path = match name {
            "MBX_CACHE_DIR" => self.mbx_cache_dir,
            "CARGO_HOME" => self.cargo_home,
            "MISE_CARGO_HOME" => self.mise_cargo_home,
            "MISE_DATA_DIR" => self.mise_data_dir,
            "RUSTUP_HOME" => self.rustup_home,
            "MISE_RUSTUP_HOME" => self.mise_rustup_home,
            "TF_PLUGIN_CACHE_DIR" => self.tofu_plugin_cache_dir,
            "MISE_TASK_CACHE_DIR" => self.mise_task_cache_dir,
            _ => None,
        }?;
        path.to_str().map(str::to_owned)
    }
}

/// Typed inputs for one qualification cache payload operation.
#[derive(Debug)]
pub struct QualificationCachePayloadContext<'request, 'environment> {
    /// Current runner facts and the plan artifact used to bind the operation.
    pub binding: &'request QualificationCacheBindingRequest<'environment>,
    /// Bounded root observations from the trusted post-setup collector.
    pub roots_json: &'request str,
    /// Current process environment paths used to validate each observation.
    pub root_environment: QualificationCacheRootEnvironment<'request>,
}

/// Action output recorded by the pinned cache action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheActionObservation {
    /// Whether the pinned cache action reports a hit.
    pub hit: bool,
    /// Exact key selected by the pinned cache action.
    pub matched_key: Option<String>,
}

/// Evidence recorded after the live roots were staged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheExportProof {
    /// Digest of the useful state copied into the stage.
    pub state_digest: String,
    /// Exact key saved by this export, if the bound policy allows a save.
    pub save_key: Option<String>,
    /// Isolated stage path written by this export, if one was created.
    pub save_stage_path: Option<PathBuf>,
    /// Digest of the checked manifest, if one was created.
    pub manifest_digest: Option<String>,
}

/// State of one cache restore operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheRestoreStatus {
    /// The plan disables this cache layer.
    Disabled,
    /// The pinned cache action reports no matching entry.
    Miss,
    /// Backend evidence and payload checks passed, then the state was installed.
    Hit,
}

/// Evidence recorded after backend and payload checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRestoreProof {
    /// Outcome after backend and payload checks.
    pub status: CacheRestoreStatus,
    /// Digest of the installed useful state, when the restore was a hit.
    pub state_digest: Option<String>,
    /// Isolated stage path used by the restore, when it was a hit.
    pub stage_path: Option<PathBuf>,
}

/// Validated qualification cache inputs. This value has no cache authority.
#[derive(Debug)]
pub struct QualificationCachePayloadExport {
    /// Binding derived from the authenticated plan and current runner facts.
    pub binding: QualificationCacheBindingOutputs,
    /// Evidence recorded by the export operation.
    pub proof: CacheExportProof,
}

/// Validated backend and payload evidence for one restore.
#[derive(Debug)]
pub struct QualificationCachePayloadImport {
    /// Binding derived from the authenticated plan and current runner facts.
    pub binding: QualificationCacheBindingOutputs,
    /// Result of the pinned cache backend census.
    pub backend: QualificationCacheBackendObservation,
    /// Evidence recorded by the import operation.
    pub proof: CacheRestoreProof,
}

/// Inputs for the normal mise-tools payload, read from the checked runtime action.
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

/// Check collector roots against the bound plan lane and runtime environment.
///
/// # Errors
///
/// Gives a static error code when the plan binding or any root path is invalid.
pub fn validate_qualification_cache_root_observation(
    request: &QualificationCacheBindingRequest<'_>,
    roots_json: &str,
    root_environment: QualificationCacheRootEnvironment<'_>,
) -> Result<(), &'static str> {
    let binding = bind_qualification_cache_runtime(request)
        .map_err(|_| "qualification_cache_binding_invalid")?;
    let roots = read_roots(request, roots_json, root_environment)?;
    transfer::engine::validate_binding_roots(&binding, &roots)
}

/// Record an isolated payload for an active qualification cache key.
///
/// # Errors
///
/// Gives a static error code when the binding is inactive, roots differ from
/// the current environment, or payload staging fails.
pub fn export_qualification_cache_payload(
    context: &QualificationCachePayloadContext<'_, '_>,
) -> Result<QualificationCachePayloadExport, &'static str> {
    let (binding, roots) = bind_context(context)?;
    let proof = transfer::engine::export_payload(&binding, &roots, context.binding.runner_temp)?;
    Ok(QualificationCachePayloadExport { binding, proof })
}

/// Check backend and staged payload evidence before a qualification restore.
///
/// # Errors
///
/// Gives a static error code when backend evidence, staged bytes, or root
/// installation fails validation.
pub fn import_qualification_cache_payload(
    context: &QualificationCachePayloadContext<'_, '_>,
    action: &CacheActionObservation,
) -> Result<QualificationCachePayloadImport, &'static str> {
    validate_action_observation(action)?;
    let (binding, roots) = bind_context(context)?;
    let observed = if let Some(key) = binding.restore_key.as_deref() {
        let workspace = workspace_path()?;
        crate::qualification_cache_backend::observe_qualification_cache_backend(
            &workspace,
            context.binding.repository,
            key,
            context.binding.git_ref,
        )
        .map_err(|_| "qualification_cache_backend_unavailable")?
    } else {
        QualificationCacheBackendObservation::NotQueried
    };
    let proof = transfer::engine::import_payload(
        &binding,
        &roots,
        context.binding.runner_temp,
        action.hit,
        action.matched_key.as_deref(),
        &observed,
    )?;
    Ok(QualificationCachePayloadImport {
        binding,
        backend: observed,
        proof,
    })
}

/// Record the normal tools payload under its full cache key.
///
/// # Errors
///
/// Gives a static error code when the runtime identity, cache key, roots, or
/// payload staging fail validation.
pub fn export_normal_tools_cache_payload(
    request: &NormalToolsCachePayloadRequest<'_>,
) -> Result<CacheExportProof, &'static str> {
    let roots = normal_tools_roots(request)?;
    transfer::engine::export_normal_payload(
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
    transfer::engine::import_normal_payload(
        request,
        &roots,
        action.hit,
        action.matched_key.as_deref(),
    )
}

fn bind_context(
    context: &QualificationCachePayloadContext<'_, '_>,
) -> Result<(QualificationCacheBindingOutputs, roots::CacheRootSet), &'static str> {
    let binding = bind_qualification_cache_runtime(context.binding)
        .map_err(|_| "qualification_cache_binding_invalid")?;
    if !binding.active {
        return Err("qualification_cache_layer_disabled");
    }
    let roots = read_roots(
        context.binding,
        context.roots_json,
        context.root_environment,
    )?;
    transfer::engine::validate_binding_roots(&binding, &roots)?;
    Ok((binding, roots))
}

fn read_roots(
    request: &QualificationCacheBindingRequest<'_>,
    roots_json: &str,
    root_environment: QualificationCacheRootEnvironment<'_>,
) -> Result<roots::CacheRootSet, &'static str> {
    let layer = roots::parse_layer(request.layer).ok_or("qualification_cache_roots_invalid")?;
    let observed = roots::CacheRootSet::parse(roots_json, request.matrix_key, layer)?;
    let expected = roots::expected_paths_for_environment(layer, request.runner_temp, |name| {
        root_environment.value(name)
    })?;
    observed.validate_expected_paths(&expected)?;
    Ok(observed)
}

fn normal_tools_roots(
    request: &NormalToolsCachePayloadRequest<'_>,
) -> Result<roots::CacheRootSet, &'static str> {
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
    let roots = roots::CacheRootSet {
        matrix_key: "normal-tools".to_owned(),
        layer: QualificationCacheLayer::MiseTools,
        roots: vec![
            roots::CacheRootObservation {
                id: roots::CacheRootId::CargoBin,
                path: request.cargo_home.join("bin"),
            },
            roots::CacheRootObservation {
                id: roots::CacheRootId::CargoCrates2Json,
                path: request.cargo_home.join(".crates2.json"),
            },
            roots::CacheRootObservation {
                id: roots::CacheRootId::CargoCratesToml,
                path: request.cargo_home.join(".crates.toml"),
            },
            roots::CacheRootObservation {
                id: roots::CacheRootId::MiseData,
                path: request.mise_data_dir.to_owned(),
            },
            roots::CacheRootObservation {
                id: roots::CacheRootId::RustupHome,
                path: request.rustup_home.to_owned(),
            },
        ],
    };
    Ok(roots)
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

fn workspace_path() -> Result<PathBuf, &'static str> {
    let path = env::var("GITHUB_WORKSPACE").map_err(|_| "missing_github_workspace")?;
    let workspace = PathBuf::from(path);
    clean_path_text(&workspace, "github_workspace_path_invalid")?;
    Ok(workspace)
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

#[cfg(test)]
#[path = "qualification_cache_payload/tests/normal_transfer_tests.rs"]
mod tests;
