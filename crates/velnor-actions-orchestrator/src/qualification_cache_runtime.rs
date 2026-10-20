//! Bind one generated cache directive to the current runner.

use std::path::Path;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::workflow::{
    QualificationCacheBackendEntry, QualificationCacheDirective, QualificationCacheLayer,
    QualificationRuntimeIdentity,
};
use velnor_actions_contract::{RunsOn, run_key_for_ci};

use crate::OrchestratorError;
use crate::internal::internal;

#[path = "qualification_cache_runtime_closure.rs"]
pub(crate) mod closure;

#[path = "qualification_cache_runtime_files.rs"]
mod files;
use self::files::{
    checked_run_dir, parse_canonical, parse_layer, read_admission, read_guest_image_projection,
    read_owned_runner_file, read_plan, validate_dispatch_context, validate_runner_temp,
    validate_runtime_environment, verified_scale_set_image_identity,
};

/// Private operation tag for binding one validated cache layer.
pub const QUALIFICATION_CACHE_BIND_OP: &str = "bind-qualification-cache-v1";

/// Fixed canonical directive artifact in the admitted run directory.
pub(crate) const QUALIFICATION_CACHE_DIRECTIVE_FILENAME: &str =
    "qualification-cache-directives.json";
/// Maximum directive bytes implied by the UTF-16 job-output budget.
///
/// The largest UTF-8 representation is three bytes per UTF-16 code unit.
pub(crate) const QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES: usize =
    crate::plan_output_limits::JOB_OUTPUTS_BUDGET_UTF16_BYTES * 3 / 2;
/// Maximum JSON passed through the runtime-identity environment value.
///
/// This stays below common per-environment-entry `exec` limits after the
/// variable name and separators are included.
const MAX_RUNTIME_IDENTITY_JSON_BYTES: usize = 65_536;

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
    /// Selected plan matrix key.
    pub matrix_key: &'a str,
    /// Closed layer wire name.
    pub layer: &'a str,
    /// Canonical runtime identity collected by the consumer job.
    pub runtime_identity_json: &'a str,
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
    /// Selected matrix key.
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
    let entry = documents
        .plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.matrix_key == request.matrix_key)
        .ok_or_else(|| internal("qualification_cache_matrix_key_missing"))?;
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
        &documents.evidence,
        &documents.plan,
        entry,
        request,
        image_identity.as_ref(),
    )?;
    let active = validate_cache_lane(&documents.directive, entry, request.matrix_key, layer)?;
    let bound = documents
        .directive
        .bind_runtime(
            &documents.plan,
            documents.admission.as_ref(),
            &entry.matrix_key,
            layer,
            &documents.evidence,
        )
        .map_err(crate::internal::internal_contract)?;
    if !active && (bound.restore.is_some() || bound.save_key.is_some()) {
        return Err(internal("qualification_cache_disabled_layer_bound"));
    }
    let restore_key = bound
        .restore
        .as_ref()
        .map(|restore| restore.requested_key.clone());
    let expected_cache = bound
        .restore
        .as_ref()
        .and_then(|restore| restore.expected_cache.clone());
    Ok(QualificationCacheBindingOutputs {
        active,
        campaign: documents.directive.campaign,
        phase: documents.directive.phase.as_str().to_owned(),
        plan_id: documents.plan.plan_id,
        run_key: documents.plan.run_key,
        matrix_key: request.matrix_key.to_owned(),
        layer: layer.as_str().to_owned(),
        restore_key,
        expected_cache,
        save_key: bound.save_key,
        save_if_state_changes: bound.save_if_state_changes,
        expected_prior_state_digest: bound.expected_prior_state_digest,
        runtime_identity_digest: bound.runtime_identity_digest,
    })
}

fn validate_cache_lane(
    directive: &QualificationCacheDirective,
    entry: &velnor_actions_contract::workflow::MatrixEntry,
    matrix_key: &str,
    layer: QualificationCacheLayer,
) -> Result<bool, OrchestratorError> {
    let lane = directive
        .lanes
        .iter()
        .find(|lane| lane.matrix_key == matrix_key)
        .ok_or_else(|| internal("qualification_cache_lane_missing"))?;
    if lane.stack_id != entry.stack_id || lane.task_id != entry.task_id {
        return Err(internal("qualification_cache_lane_identity_mismatch"));
    }
    directive
        .layer(matrix_key, layer)
        .map(|layer| layer.active)
        .ok_or_else(|| internal("qualification_cache_layer_missing"))
}

struct BindingDocuments {
    plan: velnor_actions_contract::Plan,
    admission: Option<velnor_actions_contract::workflow::QualificationCacheAdmission>,
    directive: QualificationCacheDirective,
    evidence: QualificationRuntimeIdentity,
}

fn read_binding_documents(
    request: &QualificationCacheBindingRequest<'_>,
) -> Result<BindingDocuments, OrchestratorError> {
    let run_key = run_key_for_ci(request.run_id, u64::from(request.run_attempt));
    let run_dir = checked_run_dir(request.runner_temp, &run_key)?;
    let plan = read_plan(&run_dir, &run_key)?;
    validate_dispatch_context(&plan, request)?;
    let admission_path =
        run_dir.join(crate::qualification_resolver::QUALIFICATION_ADMISSION_FILENAME);
    let admission = read_admission(&admission_path, &plan)?;
    let directive_path = run_dir.join(QUALIFICATION_CACHE_DIRECTIVE_FILENAME);
    let directive_bytes =
        read_owned_runner_file(&directive_path, QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES)?;
    let directive_text = std::str::from_utf8(&directive_bytes)
        .map_err(|_| internal("qualification_cache_directive_encoding"))?;
    let directive = parse_canonical::<QualificationCacheDirective>(
        directive_text,
        QUALIFICATION_CACHE_DIRECTIVE_MAX_BYTES,
    )?;
    let evidence = parse_canonical::<QualificationRuntimeIdentity>(
        request.runtime_identity_json,
        MAX_RUNTIME_IDENTITY_JSON_BYTES,
    )?;
    Ok(BindingDocuments {
        plan,
        admission,
        directive,
        evidence,
    })
}

#[cfg(test)]
#[path = "qualification_cache_runtime_tests.rs"]
mod tests;
