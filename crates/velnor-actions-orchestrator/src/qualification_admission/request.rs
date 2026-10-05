//! Admission request, current-run, and predecessor-resolution orchestration.

use std::env;
use std::path::{Path, PathBuf};

use serde_json::Value;
use velnor_actions_contract::{
    QualificationCacheAdmission, QualificationDispatch, QualificationPhase, WorkflowEvent,
    canonical_json_bytes, parse_strict_json,
};

use crate::OrchestratorError;
use crate::internal::{PLAN_OP, PlanRequest, check_schema, internal, internal_contract};
use crate::qualification_admission::{lineage, staged};
use crate::qualification_github_api::QualificationGitHubApi;
use crate::qualification_source_delta::source_delta;
use crate::request_event::{qualification_dispatch_for_parts, request_refs, workflow_event_for};

/// Repository whose generated workflow owns qualification admission.
const CANONICAL_VELNOR_REPOSITORY: &str = "tailrocks/velnor-new";
/// Fixed workflow path selected by the plan contract.
const CI_WORKFLOW_REF_PATH: &str = ".github/workflows/ci.yml";

/// Resolve, validate, and stage the immutable predecessor chain.
///
/// Cold/control require no predecessor and reject stale admission files.
/// Lineage phases use only typed run coordinates from the request, then
/// fetch all metadata and receipt bytes from explicitly scoped GitHub APIs.
///
/// # Errors
/// Returns an internal error for any context, API, artifact, ZIP, lineage,
/// source, or bounded-write failure.
pub fn resolve_qualification_admission(request_path: &Path) -> Result<(), OrchestratorError> {
    staged::reject_stale(request_path)?;
    let request = read_plan_request(request_path)?;
    let context = request
        .qualification
        .as_ref()
        .ok_or_else(|| internal("missing_qualification_context"))?;
    validate_request_context(&request, context)?;
    let root = current_root(&request)?;
    let api = QualificationGitHubApi::new(CANONICAL_VELNOR_REPOSITORY, &root)?;
    let (default_branch, branch_protected) = validate_current_repository(&api, context)?;
    validate_current_run(&api, context, &default_branch, branch_protected)?;
    let Some(expected_phase) = predecessor_phase(context.phase) else {
        return Ok(());
    };
    let predecessor = context
        .predecessor
        .ok_or_else(|| internal("missing_qualification_predecessor"))?;
    let root_node = lineage::resolve_node(
        &api,
        &default_branch,
        &context.repository,
        context,
        predecessor,
        expected_phase,
        None,
        1,
    )?;
    let delta = if context.phase == QualificationPhase::UsefulDelta {
        let base = lineage::node_source_sha(&root_node)?;
        Some(source_delta(&api, &root, &base, &context.source_sha)?)
    } else {
        None
    };
    let document = serde_json::json!({"predecessor": root_node, "source_delta": delta});
    let bytes = canonical_json_bytes(&document).map_err(internal_contract)?;
    QualificationCacheAdmission::parse_bounded(&bytes).map_err(internal_contract)?;
    staged::write_admission(request_path, &bytes)?;
    Ok(())
}

fn read_plan_request(path: &Path) -> Result<PlanRequest, OrchestratorError> {
    let text = crate::safe_read::read_event_file(path, crate::safe_read::MAX_REPO_FILE_BYTES)?;
    let value: Value = parse_strict_json(&text).map_err(internal_contract)?;
    let request: PlanRequest =
        serde_json::from_value(value).map_err(|_| internal("malformed_request"))?;
    check_schema(request.schema)?;
    if request.op.as_deref() != Some(PLAN_OP)
        || request.event != WorkflowEvent::Qualification
        || request.baseline_manifest.is_some()
        || request.root.as_deref() != Some(Path::new("."))
    {
        return Err(internal("qualification_request_shape_invalid"));
    }
    Ok(request)
}

fn validate_request_context(
    request: &PlanRequest,
    context: &QualificationDispatch,
) -> Result<(), OrchestratorError> {
    context
        .validate_for(
            &context.default_branch,
            CANONICAL_VELNOR_REPOSITORY,
            &request.head,
        )
        .map_err(internal_contract)?;
    if request.repository.as_deref() != Some(CANONICAL_VELNOR_REPOSITORY)
        || context.source_sha != request.head
        || context.workflow_ref
            != format!(
                "{CANONICAL_VELNOR_REPOSITORY}/{CI_WORKFLOW_REF_PATH}@{}",
                context.git_ref
            )
    {
        return Err(internal("qualification_request_context_mismatch"));
    }
    validate_runner_payload(context, request)
}

fn validate_runner_payload(
    context: &QualificationDispatch,
    request: &PlanRequest,
) -> Result<(), OrchestratorError> {
    let event_name = required_env("GITHUB_EVENT_NAME")?;
    let event_path = PathBuf::from(required_env("GITHUB_EVENT_PATH")?);
    let payload_text =
        crate::safe_read::read_event_file(&event_path, crate::safe_read::MAX_REPO_FILE_BYTES)?;
    let payload: Value =
        serde_json::from_str(&payload_text).map_err(|_| internal("malformed_event_payload"))?;
    let actual = qualification_dispatch_for_parts(
        &event_name,
        &payload,
        Some(&required_env("GITHUB_REPOSITORY")?),
        Some(&required_env("GITHUB_REF")?),
        Some(&required_env("GITHUB_REF_PROTECTED")?),
        Some(&required_env("GITHUB_WORKFLOW_REF")?),
        Some(&required_env("GITHUB_WORKFLOW_SHA")?),
        Some(&required_env("GITHUB_SHA")?),
        Some(&required_env("GITHUB_RUN_ID")?),
        Some(&required_env("GITHUB_RUN_ATTEMPT")?),
    )?;
    let event = workflow_event_for(&event_name, &payload)?;
    let (base, head) = request_refs(event, &payload, Some(&required_env("GITHUB_SHA")?))?;
    if actual.as_ref() != Some(context)
        || event != request.event
        || base != request.base
        || head != request.head
        || env::var("GH_REPO").ok().as_deref() != Some(context.repository.as_str())
        || !env::var("GH_TOKEN").is_ok_and(|token| !token.trim().is_empty())
    {
        return Err(internal("qualification_runner_context_mismatch"));
    }
    Ok(())
}

fn current_root(request: &PlanRequest) -> Result<PathBuf, OrchestratorError> {
    let cwd = env::current_dir().map_err(|err| OrchestratorError::RootDiscovery {
        problem: err.to_string(),
    })?;
    let root = crate::root::resolve_root(&cwd)?;
    if request.root.as_deref() != Some(Path::new(".")) {
        return Err(internal("qualification_checkout_root_mismatch"));
    }
    Ok(root)
}

fn validate_current_repository(
    api: &QualificationGitHubApi,
    context: &QualificationDispatch,
) -> Result<(String, bool), OrchestratorError> {
    let repository = api.repository()?;
    if repository.full_name != context.repository
        || repository.default_branch != context.default_branch
    {
        return Err(internal("qualification_current_repository_mismatch"));
    }
    let protected = api.branch_is_protected(&repository.default_branch)?;
    if !protected {
        return Err(internal("qualification_default_branch_unprotected"));
    }
    Ok((repository.default_branch, protected))
}

fn validate_current_run(
    api: &QualificationGitHubApi,
    context: &QualificationDispatch,
    default_branch: &str,
    protected: bool,
) -> Result<(), OrchestratorError> {
    let run = api.run_attempt(context.run_id, context.run_attempt, default_branch)?;
    if !protected
        || run.repository != context.repository
        || run.event != "workflow_dispatch"
        || run.status != "in_progress"
        || run.conclusion.is_some()
        || run.head_sha != context.source_sha
        || run.path_ref != format!("{CI_WORKFLOW_REF_PATH}@{default_branch}")
    {
        return Err(internal("qualification_current_run_mismatch"));
    }
    Ok(())
}

fn predecessor_phase(phase: QualificationPhase) -> Option<QualificationPhase> {
    match phase {
        QualificationPhase::Warm => Some(QualificationPhase::Cold),
        QualificationPhase::Third => Some(QualificationPhase::Warm),
        QualificationPhase::UsefulDelta => Some(QualificationPhase::Third),
        QualificationPhase::Cold | QualificationPhase::Control => None,
    }
}

fn required_env(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| internal(&format!("missing_{name}")))
}
