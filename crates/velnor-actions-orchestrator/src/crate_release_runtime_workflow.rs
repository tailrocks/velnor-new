// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseIntent, GitHubAdmissionRequest, GitRequest, OrchestratorError, Path, internal,
    is_full_sha,
};

pub(crate) fn verify_workflow_concurrency<R: crate::crate_release_registry::CrateRegistryRead>(
    workspace: &Path,
    source: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    registry: &R,
    git: &crate::crate_release_git::ReleaseGit,
    source_execution: &dyn crate::CrateSourceExecution,
) -> Result<(), OrchestratorError> {
    let effective_intent =
        crate::crate_release_controller::effective_release_intent(intent, request);
    let plan = crate::release::crate_release_workspace::crate_release_workspace_plan_with_registry(
        source,
        &request.source_sha,
        &effective_intent,
        registry,
        git,
        source_execution,
    )?;
    let bytes = read_workflow_at(workspace, &request.workflow_sha, git)?;
    validate_workflow_concurrency(&bytes, &plan.concurrency_group)
}

pub(crate) fn read_workflow_at(
    workspace: &Path,
    workflow_sha: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<u8>, OrchestratorError> {
    if !is_full_sha(workflow_sha) {
        return Err(internal("crate_release_workflow_sha"));
    }
    let show = GitRequest::show(vec![
        format!(
            "{workflow_sha}:{}",
            velnor_actions_contract::CRATE_RELEASE_WORKFLOW_PATH
        )
        .into(),
    ])
    .run_release_in(workspace, git)
    .map_err(|_| internal("crate_release_workflow_read"))?;
    if !show.success || show.stdout.is_empty() || show.stdout.len() > 1_048_576 {
        return Err(internal("crate_release_workflow_read"));
    }
    Ok(show.stdout)
}

pub(crate) fn validate_workflow_concurrency(
    bytes: &[u8],
    group: &str,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_workflow_lock_invalid");
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut options = serde_saphyr::Options::default();
    options.merge_keys = serde_saphyr::MergeKeyPolicy::Error;
    let value: serde_json::Value =
        serde_saphyr::from_str_with_options(text, options).map_err(|_| invalid())?;
    let root = value.as_object().ok_or_else(invalid)?;
    if root.contains_key("<<") {
        return Err(invalid());
    }
    let concurrency = root
        .get("concurrency")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(invalid)?;
    if concurrency.len() != 3
        || concurrency.get("group").and_then(serde_json::Value::as_str) != Some(group)
        || concurrency.get("queue").and_then(serde_json::Value::as_str) != Some("max")
        || concurrency
            .get("cancel-in-progress")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
    {
        return Err(invalid());
    }
    let lines: Vec<_> = text.lines().collect();
    let positions: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (*line == "concurrency:").then_some(index))
        .collect();
    if positions.len() != 1
        || lines.get(positions[0] + 1).copied() != Some(format!("  group: {group}").as_str())
        || lines.get(positions[0] + 2).copied() != Some("  cancel-in-progress: false")
        || lines.get(positions[0] + 3).copied() != Some("  queue: max")
    {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn is_journal_operation(operation: &str) -> bool {
    matches!(
        operation,
        "journal-start"
            | "journal-coordinate"
            | "journal-observe"
            | "journal-upload-intent"
            | "journal-repair-tag"
            | "journal-repair-release"
            | "journal-recovery-inspect"
            | "journal-reconcile-evidence"
    )
}
