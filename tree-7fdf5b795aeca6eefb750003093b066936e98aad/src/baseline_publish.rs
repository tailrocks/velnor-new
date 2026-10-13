//! Event-time `publish-baseline-v1`: trusted evidence publication.
//!
//! Runs in the `publish-baseline` job after Required passes: it binds
//! the downloaded plan to the protected-push refs, builds the exact
//! `baseline.json` later lookups consume, and stages it for the
//! artifact upload. Publication never runs inside merge consumption:
//! the merge only reads staged evidence, while this op only writes it.
//!
//! Every gate fails closed with a `publish_refused:*` reason: wrong
//! event, unprotected ref, unanchored repository, head or plan-event
//! mismatch, unverifiable generator, malformed source commit, or an
//! unproven reuse disposition. Covered obligations are skipped, never
//! carried forward: this run executed no proof for them, and a carried
//! proof without originating attestation fails validation downstream.
//! Before staging, the op self-checks the manifest through the same
//! [`validate_provenance`](crate::cover_baseline::provenance_check::validate_provenance)
//! consumers run, so publish and consume agree by construction.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use velnor_actions_contract::{WorkflowEvent, canonical_json_bytes, digest_b3, parse_strict_json};

use crate::OrchestratorError;
use crate::cover_baseline::provenance_check::{
    ProvenanceExpectations, is_unverifiable_generator_sha, validate_provenance,
};
use crate::internal::{SCHEMA, check_schema, internal, internal_contract};
use crate::internal_request::resolve_run_key;
use crate::merge::BaselineManifest;
use crate::request_event::{request_refs, workflow_event_for};

/// Publish operation tag.
pub const PUBLISH_OP: &str = "publish-baseline-v1";
/// Staged baseline filename inside the run directory.
pub(crate) const BASELINE_FILENAME: &str = "baseline.json";

/// `publish-baseline-v1` request: push refs plus protected-branch evidence.
///
/// Materialized at the request boundary from the GitHub environment;
/// the op consumes this file and reads no `GITHUB_*` ambient state
/// itself. Unknown fields (including caller-supplied manifest
/// metadata) reject via `deny_unknown_fields`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishRequest {
    /// Request schema; must be 1.
    schema: u32,
    /// Consuming operation; must be `publish-baseline-v1` when present.
    #[serde(default)]
    op: Option<String>,
    /// Triggering event; must be `push`.
    event: WorkflowEvent,
    /// Pushed head commit.
    head: String,
    /// Runner-owned repository slug (`owner/repo`) for provenance.
    #[serde(default)]
    repository: Option<String>,
    /// Pushed ref from the event payload.
    #[serde(default)]
    git_ref: Option<String>,
    /// Repository default branch from the event payload.
    #[serde(default)]
    default_branch: Option<String>,
}

/// Publish outputs: the derived artifact name for the upload step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOutputs {
    /// Derived `velnor-baseline-<commit>-<compat>` artifact name.
    pub artifact_name: String,
}

/// Materialize one canonical publish request from explicit inputs.
///
/// Parses the event payload exactly like plan requests (shared event
/// and ref resolution, so the layers can never disagree), records the
/// pushed ref plus the payload's default branch, and writes the file
/// exclusively under `anchor`. The op enforces the push gate; this
/// writer records, never judges.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed payloads,
/// unsupported events, missing refs, anchor escapes, or unwritable
/// paths.
pub(crate) fn write_publish_request(
    request_path: &Path,
    event_name: &str,
    payload_json: &str,
    github_sha: Option<&str>,
    repository: Option<&str>,
    anchor: &Path,
) -> Result<PathBuf, OrchestratorError> {
    let path = request_path.to_path_buf();
    let payload: serde_json::Value =
        serde_json::from_str(payload_json).map_err(|_| internal("malformed_event_payload"))?;
    let event = workflow_event_for(event_name, &payload)?;
    let (_, head) = request_refs(event, &payload, github_sha)?;
    let request = serde_json::json!({
        "schema": SCHEMA,
        "op": PUBLISH_OP,
        "event": event,
        "head": head,
        "repository": repository.filter(|slug| !slug.is_empty()),
        "git_ref": nonempty(payload.get("ref").and_then(serde_json::Value::as_str)),
        "default_branch": nonempty(
            payload
                .get("repository")
                .and_then(|repo| repo.get("default_branch"))
                .and_then(serde_json::Value::as_str)
        ),
    });
    let bytes = canonical_json_bytes(&request).map_err(internal_contract)?;
    if let Some(parent) = path.parent() {
        crate::exclusive_write::create_dir_no_symlink(anchor, parent)?;
    }
    crate::exclusive_write::write_exclusive(&path, &bytes, "request")?;
    Ok(path)
}

/// Trimmed non-empty string, if any.
fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// Build and stage the trusted `baseline.json` for one push.
///
/// Binds the downloaded plan to the request refs, derives the exact
/// artifact name the lookup reproduces, self-checks the manifest
/// through consumer validation, and stages it under the run
/// directory for the upload step. Returns the derived artifact name.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests,
/// refused gates, unreadable plans, and unwritable paths;
/// [`OrchestratorError::Io`] for IO failures.
pub fn baseline_publish(
    request_json: &str,
    runner_temp: &Path,
) -> Result<PublishOutputs, OrchestratorError> {
    let run_key = resolve_run_key(None)?;
    baseline_publish_to(request_json, &run_key, runner_temp)
}

/// Build and stage with an explicit run key (testable core).
///
/// The public wrapper resolves the key from the GitHub environment;
/// tests pass it explicitly for hermetic runs.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for malformed requests,
/// refused gates, unreadable plans, and unwritable paths;
/// [`OrchestratorError::Io`] for IO failures.
pub(crate) fn baseline_publish_to(
    request_json: &str,
    run_key: &str,
    runner_temp: &Path,
) -> Result<PublishOutputs, OrchestratorError> {
    velnor_actions_contract::validate_run_key(run_key).map_err(internal_contract)?;
    let request = publish_request(request_json)?;
    let (run_id, run_attempt) = ci_run_ids(run_key)?;
    publish_gate(&request)?;
    let plan = crate::task_report::load_plan(run_key, runner_temp)?;
    bind_plan(&request, &plan)?;
    let manifest = publish_manifest(&request, &plan, run_id, run_attempt)?;
    self_check(&request, &manifest)?;
    let bytes = canonical_json_bytes(&manifest).map_err(internal_contract)?;
    let path = runner_temp
        .join("velnor")
        .join(run_key)
        .join(BASELINE_FILENAME);
    crate::exclusive_write::write_exclusive(&path, &bytes, "baseline")?;
    Ok(PublishOutputs {
        artifact_name: manifest.artifact_name,
    })
}

/// Parse and validate one publish request envelope.
fn publish_request(request_json: &str) -> Result<PublishRequest, OrchestratorError> {
    let envelope = parse_strict_json(request_json).map_err(internal_contract)?;
    let request: PublishRequest =
        serde_json::from_value(envelope).map_err(|err| OrchestratorError::Internal {
            problem: format!("malformed_request:{err}"),
        })?;
    check_schema(request.schema)?;
    if request.op.as_deref().is_some_and(|op| op != PUBLISH_OP) {
        return Err(internal("op_mismatch"));
    }
    Ok(request)
}

/// Numeric run IDs from one CI run key; local runs refuse to publish.
fn ci_run_ids(run_key: &str) -> Result<(u64, u64), OrchestratorError> {
    if run_key == "local" {
        return Err(internal("publish_refused:local_run"));
    }
    run_key
        .strip_prefix('r')
        .and_then(|rest| rest.split_once("-a"))
        .and_then(|(id, attempt)| Some((id.parse::<u64>().ok()?, attempt.parse::<u64>().ok()?)))
        .ok_or_else(|| internal("bad_run_key"))
}

/// Refuse anything but a protected default-branch push.
///
/// The event must be `push`, the pushed ref must equal the payload's
/// own default branch, and the repository slug must be present and
/// well-formed. Non-push runs never reach here (the job gates them),
/// but a hand-edited workflow must still fail closed.
fn publish_gate(request: &PublishRequest) -> Result<(), OrchestratorError> {
    if request.event != WorkflowEvent::Push {
        return Err(internal("publish_refused:wrong_event"));
    }
    let protected = request
        .default_branch
        .as_deref()
        .filter(|branch| !branch.is_empty() && !branch.chars().any(char::is_whitespace))
        .map(|branch| format!("refs/heads/{branch}"));
    if protected.is_none() || request.git_ref.as_ref() != protected.as_ref() {
        return Err(internal("publish_refused:unprotected_ref"));
    }
    let anchored = request
        .repository
        .as_deref()
        .and_then(crate::origin::validate_repository_slug);
    if anchored.is_none() {
        return Err(internal("publish_refused:repository_unanchored"));
    }
    Ok(())
}

/// Bind the downloaded plan to the pushed head and event.
///
/// The plan head must equal the pushed commit and the plan event must
/// be the push itself; anything else proves a stale or forged plan
/// artifact, never this push.
fn bind_plan(
    request: &PublishRequest,
    plan: &velnor_actions_contract::Plan,
) -> Result<(), OrchestratorError> {
    if plan.head != request.head {
        return Err(internal("publish_refused:head_mismatch"));
    }
    if plan.event != WorkflowEvent::Push {
        return Err(internal("publish_refused:plan_event_mismatch"));
    }
    Ok(())
}

/// Build the trusted manifest over the plan's executed obligations.
///
/// Compatibility derives from the plan alone so later lookups name
/// this exact artifact; covered obligations are skipped (never
/// carried), and any reuse disposition refuses outright.
fn publish_manifest(
    request: &PublishRequest,
    plan: &velnor_actions_contract::Plan,
    run_id: u64,
    run_attempt: u64,
) -> Result<BaselineManifest, OrchestratorError> {
    if is_unverifiable_generator_sha(&plan.generator.sha256) {
        return Err(internal("publish_refused:generator_unverifiable"));
    }
    let mut tasks = Vec::new();
    for obligation in &plan.obligations {
        match obligation.decision {
            velnor_actions_contract::ObligationDecision::Execute => {
                tasks.push(crate::merge::required_evidence::BaselineTaskEntry {
                    task_id: obligation.task_id.clone(),
                    task_digest: obligation.task_digest.clone(),
                    input_digest: obligation.input_digest.clone(),
                    closure_digest: obligation.closure_digest.clone(),
                    proof_run_id: run_id,
                    observed_run_id: run_id,
                    external_data: None,
                    proof: None,
                });
            }
            velnor_actions_contract::ObligationDecision::CoveredByTrustedBaseline => {}
            velnor_actions_contract::ObligationDecision::ReusedFromTaskCache => {
                return Err(internal("publish_refused:unproven_reuse"));
            }
        }
    }
    tasks.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    let compat =
        crate::cover_compat::baseline_compat_for_plan(plan).map_err(|reason| internal(&reason))?;
    let name = velnor_actions_contract::artifact_id_for_baseline(&request.head, &compat)
        .map_err(|_| internal("publish_refused:bad_source_commit"))?;
    let slug = request
        .repository
        .as_deref()
        .and_then(crate::origin::validate_repository_slug)
        .ok_or_else(|| internal("publish_refused:repository_unanchored"))?;
    let git_ref = request
        .git_ref
        .clone()
        .ok_or_else(|| internal("publish_refused:unprotected_ref"))?;
    let workflow = velnor_actions_workflow_renderer::render::WORKFLOW_PATH;
    Ok(BaselineManifest {
        schema: crate::internal_plan::snapshot::CANONICAL_SCHEMA_VERSION,
        repository_id: digest_b3(format!("github.com/{slug}").as_bytes()),
        source_commit: request.head.clone(),
        ref_: git_ref.clone(),
        event: "push".to_owned(),
        workflow_ref: format!("{slug}/{workflow}@{git_ref}"),
        run_id,
        run_attempt,
        final_status: "passed".to_owned(),
        generator_version: plan.generator.version.clone(),
        generator_sha256: plan.generator.sha256.clone(),
        compatibility_id: compat,
        artifact_id: crate::cover_compat::baseline_artifact_numeric_id(&name),
        artifact_name: name,
        tasks,
        expires_at_unix: None,
    })
}

/// Self-check the staged manifest through consumer validation.
///
/// The expectations mirror the request refs exactly; a manifest this
/// check rejects would fail every consumer, so the op errors loudly
/// instead of uploading unprovable bytes.
fn self_check(
    request: &PublishRequest,
    manifest: &BaselineManifest,
) -> Result<(), OrchestratorError> {
    let slug = request
        .repository
        .as_deref()
        .and_then(crate::origin::validate_repository_slug)
        .ok_or_else(|| internal("publish_refused:repository_unanchored"))?;
    let branch = request
        .default_branch
        .clone()
        .ok_or_else(|| internal("publish_refused:unprotected_ref"))?;
    let expected = ProvenanceExpectations {
        base: request.head.clone(),
        branch,
        workflow_path: velnor_actions_workflow_renderer::render::WORKFLOW_PATH.to_owned(),
        generator_version: manifest.generator_version.clone(),
        generator_sha256: manifest.generator_sha256.clone(),
        repository_id: Some(digest_b3(format!("github.com/{slug}").as_bytes())),
        repository_slug: Some(slug),
        repository_conflict: false,
    };
    let digest = digest_b3(&canonical_json_bytes(manifest).map_err(internal_contract)?);
    validate_provenance(manifest, &digest, &expected)
        .map_err(|reason| internal(&format!("publish_self_check:{reason}")))?;
    Ok(())
}

#[cfg(test)]
mod tests;
