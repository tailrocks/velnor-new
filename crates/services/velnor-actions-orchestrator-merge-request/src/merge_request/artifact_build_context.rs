//! Run identity used to reconcile plan-declared build artifacts.

use velnor_actions_contract_workflow::ArtifactBuildRunContext;

/// Whether the authoritative plan declares provider/task build outputs.
pub(super) fn has_artifact_build_tasks(plan: &serde_json::Value) -> bool {
    plan.get("artifact_tasks")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|tasks| !tasks.is_empty())
}

/// Capture and validate immutable GitHub run identity for final reconciliation.
pub(super) fn read_artifact_build_context(
    plan: &serde_json::Value,
    run_key: &str,
    errors: &mut Vec<String>,
) -> Option<ArtifactBuildRunContext> {
    let repository_id = std::env::var("GITHUB_REPOSITORY_ID").ok();
    let repository = std::env::var("GITHUB_REPOSITORY").ok();
    let run_id = std::env::var("GITHUB_RUN_ID").ok();
    let run_attempt = std::env::var("GITHUB_RUN_ATTEMPT")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    let (Some(repository_id), Some(repository), Some(run_id), Some(run_attempt)) =
        (repository_id, repository, run_id, run_attempt)
    else {
        errors.push("missing_artifact_build_context".to_owned());
        return None;
    };
    if run_id
        .parse::<u64>()
        .ok()
        .map(|id| id.to_string())
        .as_deref()
        != Some(run_id.as_str())
        || run_attempt == 0
        || run_key != format!("r{run_id}-a{run_attempt}")
        || velnor_actions_orchestrator_core::origin::validate_repository_slug(&repository).is_none()
        || repository_id
            .parse::<u64>()
            .ok()
            .is_none_or(|id| id == 0 || id.to_string() != repository_id)
    {
        errors.push("invalid_artifact_build_context".to_owned());
        return None;
    }
    if plan.is_null() {
        errors.push("missing_artifact_build_plan".to_owned());
        return None;
    }
    Some(ArtifactBuildRunContext {
        repository_id,
        repository,
        run_id,
        run_attempt,
    })
}
