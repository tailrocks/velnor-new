//! Lenient `merge-v1` fallback: per-evidence typed parsing.
//!
//! Assembly validates staged files as JSON only (syntax plus duplicate
//! keys), so a file can be valid JSON yet fail its typed shape. Parsing
//! the whole request with one `from_value` turns one such file into a
//! request error with no diagnostic report. This fallback re-parses the
//! same envelope with per-element evidence parsing: each shape-malformed
//! staged value becomes a synthesized assembly error (mapped to closed
//! verdict tokens downstream) while the envelope itself stays strict.
//! Returns `None` when the envelope (schema, run key, inventories) is
//! malformed: those stay hard `malformed_request` errors.
//!
//! The shape mirrors [`super::MergeRequest`] exactly except that staged
//! evidence (`plan`, `matrix`, reports, baseline, attestation) stays
//! untyped; keep the two in sync. The strict path runs first, so
//! well-formed requests never reach this fallback.

use serde::Deserialize;
use velnor_actions_contract::{RequiredJobResult, VerificationScope, WorkflowEvent};

use super::MergeRequest;
use crate::cover::shard::{ResourceLimits, ShardProof};

/// `merge-v1` request with staged evidence as untyped values.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LenientRequest {
    /// Request schema; must be 1.
    schema: u32,
    /// Run key.
    run_key: String,
    /// Merge-time triggering event captured at assembly.
    #[serde(default)]
    actual_event: Option<WorkflowEvent>,
    /// Verification scope captured from the runner event.
    #[serde(default)]
    actual_scope: Option<VerificationScope>,
    /// Producer scheduling facts from the runner context.
    #[serde(default)]
    actual_producer_context: Option<velnor_actions_contract::ProducerEventContext>,
    /// Head-bound candidate attestation; required in candidate mode.
    #[serde(default)]
    candidate_attestation: Option<serde_json::Value>,
    /// Validated plan; absent when the plan artifact never landed.
    #[serde(default)]
    plan: Option<serde_json::Value>,
    /// `matrix.json` content; must agree with the plan matrix.
    #[serde(default)]
    matrix: Option<serde_json::Value>,
    /// Matrix reports to aggregate.
    matrix_reports: Vec<serde_json::Value>,
    /// Per-task report files backing every aggregate entry.
    #[serde(default)]
    task_reports: Vec<serde_json::Value>,
    /// Pre-action binding evidence.
    #[serde(default)]
    action_begins: Vec<serde_json::Value>,
    /// Terminal Action API evidence.
    #[serde(default)]
    action_reports: Vec<serde_json::Value>,
    /// Pre-execution compiled helper bindings.
    #[serde(default)]
    helper_begins: Vec<serde_json::Value>,
    /// Terminal compiled helper evidence.
    #[serde(default)]
    helper_reports: Vec<serde_json::Value>,
    /// Declared validator inventory from the workflow `needs` channel.
    required_job_ids: Vec<String>,
    /// Observed validator conclusions covering the inventory exactly.
    required_jobs: Vec<RequiredJobResult>,
    /// Terminal isolated producer evidence, parsed individually.
    #[serde(default)]
    producer_reports: Vec<serde_json::Value>,
    /// Assembly failure details; every entry fails the verdict.
    #[serde(default)]
    assembly_errors: Vec<String>,
    /// Trusted baseline manifest for coverage revalidation.
    #[serde(default)]
    baseline_manifest: Option<serde_json::Value>,
    /// Shard proofs for partitioned test entries.
    #[serde(default)]
    shard_proofs: Vec<ShardProof>,
    /// Configured resource limits revalidated here.
    #[serde(default)]
    limits: Option<ResourceLimits>,
    /// Sequential-reference obligation set.
    #[serde(default)]
    reference_task_ids: Option<Vec<String>>,
}

/// Re-parse one envelope with per-element evidence parsing.
///
/// Each shape-malformed staged value is dropped with a synthesized
/// assembly error; the caller maps those to closed verdict tokens.
/// `None` means the envelope itself is malformed (hard error).
pub(crate) fn lenient_request(envelope: &serde_json::Value) -> Option<MergeRequest> {
    let raw: LenientRequest = serde_json::from_value(envelope.clone()).ok()?;
    let mut assembly_errors = raw.assembly_errors;
    let candidate_attestation = untyped_option(
        raw.candidate_attestation,
        "unparsable_candidate_attestation",
        &mut assembly_errors,
    );
    let baseline_manifest = untyped_option(
        raw.baseline_manifest,
        "unparsable_baseline",
        &mut assembly_errors,
    );
    let plan = untyped_option(raw.plan, "unparsable_plan", &mut assembly_errors);
    let matrix = untyped_option(raw.matrix, "unparsable_matrix", &mut assembly_errors);
    let matrix_reports = untyped_list(
        raw.matrix_reports,
        "unparsable_matrix_report",
        &mut assembly_errors,
    );
    let task_reports = untyped_list(
        raw.task_reports,
        "unparsable_task_report",
        &mut assembly_errors,
    );
    let action_begins = untyped_list(
        raw.action_begins,
        "unparsable_action_begin",
        &mut assembly_errors,
    );
    let action_reports = untyped_list(
        raw.action_reports,
        "unparsable_action_report",
        &mut assembly_errors,
    );
    let helper_begins = untyped_list(
        raw.helper_begins,
        "unparsable_helper_begin",
        &mut assembly_errors,
    );
    let helper_reports = untyped_list(
        raw.helper_reports,
        "unparsable_helper_report",
        &mut assembly_errors,
    );
    let producer_reports = untyped_list(
        raw.producer_reports,
        "unparsable_producer_report",
        &mut assembly_errors,
    );
    Some(MergeRequest {
        schema: raw.schema,
        run_key: raw.run_key,
        actual_event: raw.actual_event,
        actual_scope: raw.actual_scope,
        actual_producer_context: raw.actual_producer_context,
        candidate_attestation,
        plan,
        matrix,
        matrix_reports,
        task_reports,
        action_begins,
        action_reports,
        helper_begins,
        helper_reports,
        required_job_ids: raw.required_job_ids,
        required_jobs: raw.required_jobs,
        producer_reports,
        assembly_errors,
        baseline_manifest,
        shard_proofs: raw.shard_proofs,
        limits: raw.limits,
        reference_task_ids: raw.reference_task_ids,
    })
}

/// Parse one optional staged value; shape failures become an error entry.
fn untyped_option<T>(
    value: Option<serde_json::Value>,
    kind: &str,
    errors: &mut Vec<String>,
) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let value = value?;
    if let Ok(item) = serde_json::from_value(value) {
        Some(item)
    } else {
        errors.push(kind.to_owned());
        None
    }
}

/// Parse staged report values; each shape failure becomes an error entry.
fn untyped_list<T>(values: Vec<serde_json::Value>, kind: &str, errors: &mut Vec<String>) -> Vec<T>
where
    T: serde::de::DeserializeOwned,
{
    values
        .into_iter()
        .filter_map(|value| {
            if let Ok(item) = serde_json::from_value(value) {
                Some(item)
            } else {
                errors.push(kind.to_owned());
                None
            }
        })
        .collect()
}
