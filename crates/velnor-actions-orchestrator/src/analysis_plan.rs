//! Early authenticated inventory admission and the common planner boundary.

use std::path::{Path, PathBuf};

#[path = "analysis_admission.rs"]
mod admission;

use velnor_actions_contract::{
    ReleaseManifest, VerificationScope, WorkflowPolicy, parse_strict_json,
};
use velnor_actions_mise::{RuntimePaths, ToolCatalog};

use super::analysis_publication::{
    AnalysisPublicationAttempt, AnalysisPublicationOutputs, stage_analysis,
};
use super::{PlanRequest, check_schema, internal, internal_contract, plan_prepared, plan_root};
use crate::OrchestratorError;
use crate::analysis_inventory::authority::{
    AnalysisLookupInputs, AuthenticatedAnalysis, retrieve_analysis,
};
use crate::analysis_inventory::parse_authenticated;
use crate::config::load_config;
use crate::discover_index::build_file_index;
use crate::internal_request::resolve_run_key;
use crate::inventory::InventoryProvider;
use crate::prepare::{prepare, prepare_with_inventory};
use crate::select::verify_checkout;
use crate::validators::validate_diff_rev;

/// Early lookup either produces a normal plan or explicitly requires Cargo.
#[derive(Debug)]
pub enum EarlyPlanResult {
    /// Authenticated current inventory drove the full normal planner.
    Ready {
        /// Exactly the normal schema-1 plan response.
        response: String,
    },
    /// No reusable inventory; the caller must install Cargo and plan freshly.
    NeedsCargo {
        /// Explicit inventory admission failure, never a masked planner failure.
        reason: String,
    },
}

/// Compute a fresh normal plan and export qualified analysis when requested.
///
/// # Errors
/// Returns normal planning or publication errors.
pub fn plan_internal_with_analysis(
    request_json: &str,
    runner_temp: &Path,
) -> Result<(String, Option<AnalysisPublicationOutputs>), OrchestratorError> {
    plan_fresh(request_json, Some(runner_temp))
}

pub(super) fn plan_fresh(
    request_json: &str,
    runner_temp: Option<&Path>,
) -> Result<(String, Option<AnalysisPublicationOutputs>), OrchestratorError> {
    let (mut request, root) = resolved_request(request_json, super::PLAN_OP)?;
    let prep = prepare(&root)?;
    if prep.config.workflow.policy == WorkflowPolicy::ConsumerV1
        && let Some(context) = request.producer_context.as_mut()
    {
        context.cargo_fallback = true;
    }
    let mut response = plan_prepared(request.clone(), &prep)?;
    check_freshness(&prep)?;
    let attempt = match runner_temp {
        Some(temp) => stage_analysis(
            &prep,
            &request.head,
            request.publication_context.as_ref(),
            temp,
        )?,
        None => AnalysisPublicationAttempt::NotRequested,
    };
    let publication = match attempt {
        AnalysisPublicationAttempt::NotRequested => None,
        AnalysisPublicationAttempt::Published(outputs) => Some(outputs),
        AnalysisPublicationAttempt::Unavailable { reason } => {
            response = publication_warning(&response, &reason)?;
            None
        }
    };
    Ok((response, publication))
}

/// Try planning before Cargo setup using remotely authenticated exact-base data.
///
/// # Errors
/// Malformed requests, policy, checkout, config and ordinary planner failures
/// remain hard failures. Only typed inventory admission misses request Cargo.
pub fn plan_early_internal(request_json: &str) -> Result<EarlyPlanResult, OrchestratorError> {
    let (request, root) = resolved_request(request_json, super::PLAN_OP)?;
    match early_plan(request, &root) {
        Ok(response) => Ok(EarlyPlanResult::Ready { response }),
        Err(OrchestratorError::NeedsCargo { problem }) => {
            Ok(EarlyPlanResult::NeedsCargo { reason: problem })
        }
        Err(error) => Err(error),
    }
}

fn early_plan(request: PlanRequest, root: &Path) -> Result<String, OrchestratorError> {
    early_plan_with_lookup(request, root, retrieve_analysis)
}

fn early_plan_with_lookup(
    request: PlanRequest,
    root: &Path,
    lookup: impl FnOnce(AnalysisLookupInputs<'_>) -> Result<AuthenticatedAnalysis, String>,
) -> Result<String, OrchestratorError> {
    let config = load_config(root)?;
    if config.workflow.policy != WorkflowPolicy::ConsumerV1 {
        return Err(internal("early_requires_consumer_policy"));
    }
    let helper_sha256 = verified_helper(root)?;
    let (index, skipped) = build_file_index(root, &config.discovery.exclude)?;
    if skipped {
        return Err(needs_cargo("analysis_non_utf8_inputs"));
    }
    if let Some(response) = admission::plan_without_rust(&request, root, &config, &index)? {
        return Ok(response);
    }
    let base = request
        .base
        .as_deref()
        .ok_or_else(|| needs_cargo("analysis_base_absent"))?;
    let branch = crate::prepare::resolve_default_branch(root, &config)?;
    let catalog = ToolCatalog::pinned();
    let downloaded = lookup(AnalysisLookupInputs {
        catalog: &catalog,
        root,
        base,
        workflow: velnor_actions_workflow_renderer::render::WORKFLOW_PATH,
        branch: &branch,
        repository: request.repository.as_deref(),
        helper_sha256: &helper_sha256,
    })
    .map_err(|problem| OrchestratorError::NeedsCargo { problem })?;
    let inventory =
        parse_authenticated(root, index.files(), &downloaded.text, &downloaded.authority)
            .map_err(|problem| OrchestratorError::NeedsCargo { problem })?;
    let prep = prepare_with_inventory(root, InventoryProvider::ValidatedInventory(&inventory))?;
    let response = plan_prepared(request, &prep)?;
    require_plan_rust_covered(&response)?;
    check_freshness(&prep)?;
    Ok(response)
}

fn needs_cargo(problem: &str) -> OrchestratorError {
    OrchestratorError::NeedsCargo {
        problem: problem.to_owned(),
    }
}

fn verified_helper(root: &Path) -> Result<String, OrchestratorError> {
    let text = crate::discover::read_manifest_file(root)?
        .ok_or_else(|| internal("consumer_requires_release_install"))?;
    let manifest = ReleaseManifest::parse_json(&text, "release-manifest.json")?;
    manifest.validate("release-manifest.json")?;
    if manifest.version != env!("CARGO_PKG_VERSION") {
        return Err(internal("analysis_helper_version_mismatch"));
    }
    let helper = crate::cover_identity::generator::current_exe_sha256()
        .ok_or_else(|| internal("analysis_helper_unverifiable"))?;
    let target = running_target().ok_or_else(|| internal("analysis_helper_target_unsupported"))?;
    if !manifest
        .record_for_target(target)
        .is_some_and(|record| record.sha256 == helper)
    {
        return Err(internal("analysis_helper_digest_mismatch"));
    }
    Ok(helper)
}

fn running_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

fn resolved_request(
    request_json: &str,
    operation: &str,
) -> Result<(PlanRequest, PathBuf), OrchestratorError> {
    let envelope = parse_strict_json(request_json).map_err(internal_contract)?;
    let mut request: PlanRequest =
        serde_json::from_value(envelope).map_err(|err| OrchestratorError::Internal {
            problem: format!("malformed_request:{err}"),
        })?;
    check_schema(request.schema)?;
    if request.op.as_deref().is_some_and(|op| op != operation) {
        return Err(internal("op_mismatch"));
    }
    request.run_key = resolve_run_key(Some(request.run_key.as_str()))?;
    if request.head.trim().is_empty() {
        return Err(internal("empty_head"));
    }
    let root = plan_root(request.root.as_deref())?;
    if request.scope == VerificationScope::Full {
        if let Some(base) = request.base.as_deref() {
            validate_diff_rev(base, "bad_base").map_err(|problem| internal(&problem))?;
        }
    }
    request.head = verify_checkout(&root, request.event, request.base.as_deref(), &request.head)?;
    if request.scope == VerificationScope::Full {
        request.base = None;
    }
    Ok((request, root))
}

/// Revalidate an early normal response immediately before final publication.
///
/// # Errors
/// Rejects stale requests, malformed plans, mismatched matrix or helper identity.
pub fn validate_early_response(
    request_json: &str,
    response_json: &str,
) -> Result<String, OrchestratorError> {
    validate_with_lookup(request_json, response_json, retrieve_analysis)
}

fn validate_with_lookup(
    request_json: &str,
    response_json: &str,
    lookup: impl FnOnce(AnalysisLookupInputs<'_>) -> Result<AuthenticatedAnalysis, String>,
) -> Result<String, OrchestratorError> {
    let (request, root) = resolved_request(request_json, super::PLAN_OP)?;
    let helper = verified_helper(&root)?;
    let value = parse_strict_json(response_json).map_err(internal_contract)?;
    let response: super::PlanResponse = serde_json::from_value(value)
        .map_err(|error| internal(&format!("early_response_decode:{error}")))?;
    check_schema(response.schema)?;
    let plan = &response.plan;
    let generator = crate::internal_plan::default_generator();
    plan.validate().map_err(internal_contract)?;
    if plan.run_key != request.run_key
        || plan.head != request.head
        || plan.base != request.base
        || plan.event != request.event
        || plan.scope != request.scope
        || plan.generator.sha256 != helper
        || plan.generator.version != generator.version
        || plan.generator.target != generator.target
        || velnor_actions_contract::canonical_json_bytes(&response.matrix)
            .map_err(internal_contract)?
            != velnor_actions_contract::canonical_json_bytes(&plan.matrix)
                .map_err(internal_contract)?
    {
        return Err(internal("early_response_request_mismatch"));
    }
    super::check_matrix_budget(&response.matrix)?;
    let recomputed = early_plan_with_lookup(request, &root, lookup)?;
    let current = parse_strict_json(&recomputed).map_err(internal_contract)?;
    let staged = serde_json::to_value(&response)
        .map_err(|error| internal(&format!("response_encode:{error}")))?;
    if velnor_actions_contract::canonical_json_bytes(&current).map_err(internal_contract)?
        != velnor_actions_contract::canonical_json_bytes(&staged).map_err(internal_contract)?
    {
        return Err(internal("early_response_stale"));
    }
    Ok(recomputed)
}

fn check_freshness(prep: &crate::prepare::GenerationPreparation) -> Result<(), OrchestratorError> {
    if std::env::var("VELNOR_PLAN_FRESHNESS").as_deref() != Ok("1") {
        return Ok(());
    }
    check_freshness_prepared(prep)
}

fn check_freshness_prepared(
    prep: &crate::prepare::GenerationPreparation,
) -> Result<(), OrchestratorError> {
    let preview = tempfile::tempdir()
        .map_err(|error| OrchestratorError::io("freshness_preview", error.to_string()))?;
    crate::generate::generate_in_runtime(
        prep,
        &crate::generate::GenerateOptions {
            output_dir: Some(preview.path().to_path_buf()),
        },
        plan_runtime(prep),
    )?;
    let output = velnor_actions_mise::GitRequest::diff(vec![
        "--no-index".into(),
        "--exit-code".into(),
        "--".into(),
        prep.root.join(".github").into_os_string(),
        preview.path().join(".github").into_os_string(),
    ])
    .run_in(&prep.root)
    .map_err(|error| internal(&format!("freshness_diff:{error}")))?;
    if !output.success {
        return Err(internal("generated_files_stale"));
    }
    Ok(())
}

fn require_plan_rust_covered(response: &str) -> Result<(), OrchestratorError> {
    let value = parse_strict_json(response).map_err(internal_contract)?;
    let response: super::PlanResponse = serde_json::from_value(value)
        .map_err(|error| internal(&format!("response_decode:{error}")))?;
    if response.plan.obligations.iter().any(|obligation| {
        obligation.job_id == velnor_actions_contract::PLAN_JOB_ID
            && obligation.task_id.starts_with("stack/rust/")
            && obligation.decision
                != velnor_actions_contract::ObligationDecision::CoveredByTrustedBaseline
    }) {
        return Err(needs_cargo("plan_rust_obligation_uncovered"));
    }
    Ok(())
}

#[cfg(test)]
fn test_plan_early(
    request_json: &str,
    lookup: impl FnOnce(AnalysisLookupInputs<'_>) -> Result<AuthenticatedAnalysis, String>,
) -> Result<EarlyPlanResult, OrchestratorError> {
    let (request, root) = resolved_request(request_json, super::PLAN_OP)?;
    match early_plan_with_lookup(request, &root, lookup) {
        Ok(response) => Ok(EarlyPlanResult::Ready { response }),
        Err(OrchestratorError::NeedsCargo { problem }) => {
            Ok(EarlyPlanResult::NeedsCargo { reason: problem })
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
fn test_check_freshness(
    prep: &crate::prepare::GenerationPreparation,
) -> Result<(), OrchestratorError> {
    check_freshness_prepared(prep)
}

/// Read only a bounded regular staged response, then reauthenticate its plan.
///
/// # Errors
/// Rejects missing, symlinked, malformed, stale or unauthenticated responses.
pub fn read_early_response(
    request_json: &str,
    response_path: &Path,
) -> Result<String, OrchestratorError> {
    let text = crate::retrieve_reports::read_staged_text(response_path, 8 * 1024 * 1024)
        .map_err(|problem| internal(&format!("early_response_read:{problem}")))?;
    validate_early_response(request_json, &text)
}

#[cfg(test)]
#[path = "analysis_plan_tests.rs"]
mod tests;

fn publication_warning(response: &str, reason: &str) -> Result<String, OrchestratorError> {
    let value = parse_strict_json(response).map_err(internal_contract)?;
    let mut response: super::PlanResponse = serde_json::from_value(value)
        .map_err(|error| internal(&format!("response_decode:{error}")))?;
    response
        .plan
        .warnings
        .push(format!("analysis_publication_unavailable:{reason}"));
    response.plan.warnings.sort();
    response.plan.warnings.dedup();
    serde_json::to_string(&response).map_err(|error| internal(&format!("response_encode:{error}")))
}

#[cfg(test)]
fn test_validate_early(
    request_json: &str,
    response_json: &str,
    lookup: impl FnOnce(AnalysisLookupInputs<'_>) -> Result<AuthenticatedAnalysis, String>,
) -> Result<String, OrchestratorError> {
    validate_with_lookup(request_json, response_json, lookup)
}

/// Consumer Plan tools have one compiled root; source dogfood uses full tools.
pub(super) const fn plan_runtime(prep: &crate::prepare::GenerationPreparation) -> RuntimePaths {
    match prep.config.workflow.policy {
        WorkflowPolicy::ConsumerV1 => RuntimePaths::planning(),
        WorkflowPolicy::VelnorRepositoryV1 => RuntimePaths::full(),
    }
}
