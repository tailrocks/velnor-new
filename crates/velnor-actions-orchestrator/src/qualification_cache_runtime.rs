//! Bind one generated cache directive to the current runner.

use std::path::Path;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheDirective, QualificationRuntimeIdentity,
};
use velnor_actions_contract::{RunsOn, run_key_for_ci};

use crate::OrchestratorError;
use crate::internal::internal;

#[path = "qualification_cache_runtime_closure.rs"]
pub(crate) mod closure;

#[path = "qualification_cache_runtime_before.rs"]
mod before;
#[path = "qualification_cache_runtime_files.rs"]
mod files;
#[path = "qualification_cache_runtime_names.rs"]
mod names;
#[path = "qualification_cache_runtime_observation.rs"]
mod observation;
#[path = "qualification_cache_runtime_observer_writer.rs"]
mod observer_writer;
#[path = "qualification_cache_runtime_owner_evidence.rs"]
mod owner_evidence;
#[cfg(test)]
use self::files::read_owned_runner_file;
use self::files::{
    RunDirectory, parse_layer, read_guest_image_projection, validate_runner_temp,
    validate_runtime_environment, verified_scale_set_image_identity,
};
pub use self::owner_evidence::{
    aggregate_qualification_cache_worker_owner_facts, write_qualification_cache_worker_layer_facts,
};

/// Private operation tag for binding one validated cache layer.
pub const QUALIFICATION_CACHE_BIND_OP: &str = "bind-qualification-cache-v1";
/// Private operation tag for observing generated owner roots.
pub const QUALIFICATION_CACHE_ROOTS_OP: &str = "observe-qualification-cache-roots-v1";

/// Fixed canonical directive artifact in the admitted run directory.
pub(crate) const QUALIFICATION_CACHE_DIRECTIVE_FILENAME: &str =
    "qualification-cache-directives.json";
/// Maximum directive bytes implied by the UTF-16 job-output budget.
///
/// The largest UTF-8 representation is three bytes per UTF-16 code unit.
pub(crate) const QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES: usize =
    crate::plan_output_limits::JOB_OUTPUTS_BUDGET_UTF16_BYTES * 3 / 2;
/// Maximum canonical JSON stored in the runtime-identity file.
const MAX_RUNTIME_IDENTITY_JSON_BYTES: usize = 65_536;
const MAX_ROOTS_JSON_BYTES: usize = 65_536;

/// Current runner facts checked against the staged qualification plan.
#[derive(Debug, Clone)]
pub struct QualificationCacheBindingRequest<'a> {
    /// Runner temporary root.
    pub runner_temp: &'a Path,
    /// Actual run identifier.
    pub run_id: u64,
    /// Actual run attempt.
    pub run_attempt: u32,
    /// Actual event name.
    pub event_name: &'a str,
    /// Actual repository slug.
    pub repository: &'a str,
    /// Actual full Git ref.
    pub git_ref: &'a str,
    /// Actual protected-ref flag.
    pub ref_protected: bool,
    /// Actual workflow ref.
    pub workflow_ref: &'a str,
    /// Actual workflow source revision.
    pub workflow_sha: &'a str,
    /// Actual checked-out source revision.
    pub source_sha: &'a str,
    /// Actual runner operating system.
    pub runner_os: &'a str,
    /// Actual runner architecture.
    pub runner_arch: &'a str,
    /// Verified runner image family.
    pub image_os: Option<&'a str>,
    /// Exact generated job that owns the physical cache layer.
    pub owner_id: &'a str,
    /// Closed layer wire name.
    pub layer: &'a str,
    /// Exact run-owned runtime identity file collected by the consumer job.
    pub runtime_identity_path: &'a Path,
}

/// Exact keys and predecessor evidence for one cache layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheBindingOutputs {
    /// Whether the typed directive activates this layer.
    pub active: bool,
    /// Campaign namespace from the validated plan.
    pub campaign: String,
    /// Phase from the validated plan.
    pub phase: String,
    /// Current plan identifier.
    pub plan_id: String,
    /// Current run key.
    pub run_key: String,
    /// Exact generated job that owns this cache layer.
    pub owner_id: String,
    /// Representative plan matrix key for legacy report bookkeeping.
    pub matrix_key: String,
    /// Selected layer wire name.
    pub layer: String,
    /// Exact primary restore key, when restore is enabled.
    pub restore_key: Option<String>,
    /// Exact predecessor cache entry, or null when a miss is required.
    pub expected_cache: Option<QualificationCacheBackendEntry>,
    /// Exact save key, when saving is enabled.
    pub save_key: Option<String>,
    /// Whether save is conditional on a changed useful-state digest.
    pub save_if_state_changes: bool,
    /// Admitted predecessor useful-state digest, when present.
    pub expected_prior_state_digest: Option<String>,
    /// Digest of the observed runtime identity used in the keys.
    pub runtime_identity_digest: Option<String>,
}

/// Bind a directive to an exact plan lane and runner identity.
///
/// The serialized directive cannot grant access. The function rebuilds it
/// from the validated plan and admitted predecessor before returning keys.
/// # Errors
pub fn bind_qualification_cache_runtime(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<QualificationCacheBindingOutputs, OrchestratorError> {
    validate_runner_temp(request.runner_temp)?;
    let layer = parse_layer(request.layer)?;
    let documents = read_binding_documents(request)?;
    velnor_actions_contract::workflow::validate_job_id(request.owner_id)
        .map_err(crate::internal::internal_contract)?;
    let entry = documents
        .plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.job_id == request.owner_id)
        .ok_or_else(|| internal("qualification_cache_owner_missing"))?;
    let active = documents
        .directive
        .owner_layer(request.owner_id, layer)
        .map(|record| record.active)
        .ok_or_else(|| internal("qualification_cache_owner_layer_missing"))?;
    let expected_directive =
        QualificationCacheDirective::for_plan(&documents.plan, documents.admission.as_ref())
            .map_err(crate::internal::internal_contract)?
            .ok_or_else(|| internal("qualification_cache_plan_directive_missing"))?;
    if documents.directive != expected_directive {
        return Err(internal("qualification_cache_directive_mismatch"));
    }
    if !active {
        return Ok(disabled_binding(request, &documents, entry, layer));
    }
    let evidence = documents
        .evidence
        .as_ref()
        .ok_or_else(|| internal("qualification_cache_runtime_identity_missing"))?;
    bind_active_runtime(request, &documents, entry, layer, evidence)
}

pub use before::qualification_cache_before_observation;

fn disabled_binding(
    request: &QualificationCacheBindingRequest<'_>,
    documents: &documents::BindingDocuments,
    entry: &velnor_actions_contract::MatrixEntry,
    layer: velnor_actions_contract::workflow::QualificationCacheLayer,
) -> QualificationCacheBindingOutputs {
    QualificationCacheBindingOutputs {
        active: false,
        campaign: documents.directive.campaign.clone(),
        phase: documents.directive.phase.as_str().to_owned(),
        plan_id: documents.plan.plan_id.clone(),
        run_key: documents.plan.run_key.clone(),
        owner_id: request.owner_id.to_owned(),
        matrix_key: entry.matrix_key.clone(),
        layer: layer.as_str().to_owned(),
        restore_key: None,
        expected_cache: None,
        save_key: None,
        save_if_state_changes: false,
        expected_prior_state_digest: None,
        runtime_identity_digest: None,
    }
}

fn bind_active_runtime(
    request: &QualificationCacheBindingRequest<'_>,
    documents: &documents::BindingDocuments,
    entry: &velnor_actions_contract::MatrixEntry,
    layer: velnor_actions_contract::workflow::QualificationCacheLayer,
    evidence: &QualificationRuntimeIdentity,
) -> Result<QualificationCacheBindingOutputs, OrchestratorError> {
    let image_identity = match RunsOn::parse(&entry.planned_platform.runs_on)
        .map_err(crate::internal::internal_contract)?
    {
        RunsOn::Hosted(_) => None,
        RunsOn::ScaleSet(selector) => {
            let expected_source_sha = documents
                .plan
                .qualification
                .as_ref()
                .and_then(|context| context.scale_set_image_source_sha.as_deref())
                .ok_or_else(|| internal("qualification_guest_image_source_authority_missing"))?;
            let projection = read_guest_image_projection()?;
            Some(verified_scale_set_image_identity(
                &projection,
                expected_source_sha,
                &selector,
            )?)
        }
    };
    validate_runtime_environment(
        evidence,
        &documents.plan,
        entry,
        request,
        image_identity.as_ref(),
    )?;
    let bound = documents
        .directive
        .bind_runtime_owner(
            &documents.plan,
            documents.admission.as_ref(),
            &entry.job_id,
            layer,
            evidence,
        )
        .map_err(crate::internal::internal_contract)?;
    let restore_key = bound
        .restore
        .as_ref()
        .map(|restore| restore.requested_key.clone());
    let expected_cache = bound
        .restore
        .as_ref()
        .and_then(|restore| restore.expected_cache.clone());
    Ok(QualificationCacheBindingOutputs {
        active: true,
        campaign: documents.directive.campaign.clone(),
        phase: documents.directive.phase.as_str().to_owned(),
        plan_id: documents.plan.plan_id.clone(),
        run_key: documents.plan.run_key.clone(),
        owner_id: request.owner_id.to_owned(),
        matrix_key: entry.matrix_key.clone(),
        layer: layer.as_str().to_owned(),
        restore_key,
        expected_cache,
        save_key: bound.save_key,
        save_if_state_changes: bound.save_if_state_changes,
        expected_prior_state_digest: bound.expected_prior_state_digest,
        runtime_identity_digest: bound.runtime_identity_digest,
    })
}

/// Observe and persist actual runtime identity for one generated owner.
///
/// # Errors
/// Returns an error when runner context, tool observations, or owner binding is invalid.
pub fn observe_qualification_cache_runtime<E>(
    request: &QualificationCacheBindingRequest<'_>,
    get: E,
) -> Result<String, OrchestratorError>
where
    E: FnMut(&str) -> Option<String>,
{
    observer_writer::observe_and_write(request, get)
}

/// Observe the runtime after import and require the same identity as before import.
///
/// # Errors
/// Returns an error if the runtime changes or the pinned identity record is invalid.
pub fn observe_qualification_cache_runtime_after_import<E>(
    request: &QualificationCacheBindingRequest<'_>,
    get: E,
) -> Result<(String, bool), OrchestratorError>
where
    E: FnMut(&str) -> Option<String>,
{
    observer_writer::observe_after_import(request, get)
}

#[path = "qualification_cache_runtime_documents.rs"]
mod documents;
use documents::read_binding_documents;

/// Return the owner-and-layer-specific runtime identity filename.
///
/// # Errors
/// Returns an error when the owner ID or layer is outside its closed contract.
pub use names::{
    qualification_cache_runtime_after_import_filename,
    qualification_cache_runtime_identity_filename,
};

pub use owner_evidence::{
    QualificationCacheOwnerLayerContext, aggregate_qualification_cache_owner_evidence,
    qualification_cache_owner_layer_context, read_qualification_cache_owner_evidence,
    write_qualification_cache_owner_layer_evidence,
};

/// Write one canonical environment-derived root record into the pinned run directory.
///
/// # Errors
/// Returns an error when owner context or configured root paths are invalid.
pub fn write_qualification_cache_roots<E>(
    runner_temp: &Path,
    run_id: u64,
    run_attempt: u32,
    owner_id: &str,
    layer: &str,
    get: E,
) -> Result<String, OrchestratorError>
where
    E: FnMut(&str) -> Option<String>,
{
    let run_key = run_key_for_ci(run_id, u64::from(run_attempt));
    let bytes =
        crate::qualification_cache_payload::collect_owner_roots(runner_temp, owner_id, layer, get)
            .map_err(internal)?;
    if bytes.len() > MAX_ROOTS_JSON_BYTES {
        return Err(internal("qualification_cache_roots_oversize"));
    }
    let filename = qualification_cache_roots_filename(owner_id, layer)?;
    let run_dir = RunDirectory::open(runner_temp, &run_key)?;
    run_dir.write_owned_file(&filename, bytes.as_bytes(), "qualification_cache_roots")?;
    Ok(runner_temp
        .join("velnor")
        .join(run_key)
        .join(filename)
        .display()
        .to_string())
}

/// Read the owner-root file only from the run directory derived from context.
///
/// # Errors
/// Returns an error when the path, owner context, or root record is invalid.
pub fn read_qualification_cache_roots(
    request: &QualificationCacheBindingRequest<'_>,
    path: &Path,
) -> Result<String, OrchestratorError> {
    let layer = parse_layer(request.layer)?;
    let run_key = run_key_for_ci(request.run_id, u64::from(request.run_attempt));
    let filename = qualification_cache_roots_filename(request.owner_id, layer.as_str())?;
    let expected = request
        .runner_temp
        .join("velnor")
        .join(&run_key)
        .join(&filename);
    if path != expected {
        return Err(internal("qualification_cache_roots_path"));
    }
    let run_dir = RunDirectory::open(request.runner_temp, &run_key)?;
    let bytes = run_dir.read_owned_file(
        &filename,
        MAX_ROOTS_JSON_BYTES,
        Some(0o600),
        "qualification_cache_roots",
    )?;
    String::from_utf8(bytes).map_err(|_| internal("qualification_cache_roots_encoding"))
}

/// Return a validated owner/layer-specific root file name.
///
/// # Errors
/// Returns an error when the owner ID or layer is outside its closed contract.
pub fn qualification_cache_roots_filename(
    owner_id: &str,
    layer: &str,
) -> Result<String, OrchestratorError> {
    velnor_actions_contract::workflow::validate_job_id(owner_id)
        .map_err(crate::internal::internal_contract)?;
    let layer = parse_layer(layer)?;
    Ok(format!("cache-roots-{}-{}.json", owner_id, layer.as_str()))
}

#[cfg(test)]
#[path = "qualification_cache_runtime_tests.rs"]
mod tests;
