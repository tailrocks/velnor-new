// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseAdmissionReceipt, CrateReleaseIntent, Duration, GitHubAdmissionOutcome,
    GitHubAdmissionRequest, Instant, NativeGitHubAdmissionRead, OpenOptions, OrchestratorError,
    Path, PathBuf, Write, admit_github_evidence, env, internal, thread,
};

pub(super) fn request_from_environment(
    intent: &CrateReleaseIntent,
    workflow_sha: &str,
) -> Result<GitHubAdmissionRequest, OrchestratorError> {
    let repository = required("GITHUB_REPOSITORY")?;
    let event = required("GITHUB_EVENT_NAME")?;
    let git_ref = required("GITHUB_REF")?;
    let workflow_ref = required("GITHUB_WORKFLOW_REF")?;
    let workflow_commit = required("GITHUB_WORKFLOW_SHA")?;
    if repository != intent.repository
        || git_ref != format!("refs/heads/{}", intent.default_branch)
        || workflow_ref != format!("{repository}/.github/workflows/release.yml@{git_ref}")
        || workflow_commit != workflow_sha
    {
        return Err(internal("crate_release_workflow_identity"));
    }
    if intent.new_crates.is_some() && event != "workflow_dispatch" {
        return Err(internal("crate_release_creation_requires_dispatch"));
    }
    let recovery_sha = if event == "workflow_dispatch" {
        Some(
            intent
                .recovery
                .as_ref()
                .map(|item| &item.source_sha)
                .or_else(|| intent.new_crates.as_ref().map(|item| &item.source_sha))
                .ok_or_else(|| internal("crate_release_recovery_missing"))?
                .clone(),
        )
    } else if event == "push" {
        None
    } else {
        return Err(internal("crate_release_event_invalid"));
    };
    let source_sha = recovery_sha
        .clone()
        .unwrap_or_else(|| workflow_sha.to_owned());
    let run_id = required("GITHUB_RUN_ID")?
        .parse()
        .map_err(|_| internal("crate_release_run_id"))?;
    let run_attempt = required("GITHUB_RUN_ATTEMPT")?
        .parse()
        .map_err(|_| internal("crate_release_run_attempt"))?;
    Ok(GitHubAdmissionRequest {
        repository,
        default_branch: intent.default_branch.clone(),
        environment: intent.environment.clone(),
        workflow_path: ".github/workflows/release.yml".to_owned(),
        required_workflow_path: intent.required_workflow_path.clone(),
        trusted_reviewer_id: intent.trusted_reviewer_id,
        workflow_sha: workflow_sha.to_owned(),
        run_id,
        run_attempt,
        event,
        source_sha,
        recovery_sha,
    })
}

pub(super) fn source_admit(
    request: &GitHubAdmissionRequest,
    token: &str,
    require_candidate: bool,
) -> Result<(), OrchestratorError> {
    let deadline = Instant::now() + Duration::from_secs(600);
    let outcome = poll_pending_admission(
        || {
            let github = NativeGitHubAdmissionRead::new(token.to_owned())?;
            admit_github_evidence(request, &github)
        },
        || Instant::now() < deadline,
        || thread::sleep(Duration::from_secs(5)),
    )?;
    match outcome {
        GitHubAdmissionOutcome::Candidate(_) => {
            let approval = format!(
                "I reviewed the complete release policy at {} and source {}, and authorize this source for run {}.",
                request.workflow_sha, request.source_sha, request.run_id
            );
            println!(
                "Review the release policy and approve the protected environment with this comment: {approval}"
            );
            append_output(&format!(
                "eligible=true\nsource_sha={}\napproval_comment={approval}\n",
                request.source_sha
            ))
        }
        GitHubAdmissionOutcome::Skipped if require_candidate => {
            Err(internal("crate_release_candidate_disappeared"))
        }
        GitHubAdmissionOutcome::Skipped => append_output("eligible=false\n"),
    }
}

pub(super) fn read_receipt(path: &Path) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let text = crate::safe_read::read_event_file(path, 1_048_576)
        .map_err(|_| internal("crate_release_receipt_read"))?;
    serde_json::from_str(&text).map_err(|_| internal("crate_release_receipt_invalid"))
}

pub(super) fn required(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("crate_release_missing_{name}")))
}
pub(super) fn required_path(name: &str) -> Result<PathBuf, OrchestratorError> {
    let path = PathBuf::from(required(name)?);
    if !path.is_absolute() {
        return Err(internal("crate_release_path_invalid"));
    }
    Ok(path)
}
pub(super) fn append_output(body: &str) -> Result<(), OrchestratorError> {
    let path = required_path("GITHUB_OUTPUT")?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|_| internal("crate_release_output"))?;
    file.write_all(body.as_bytes())
        .map_err(|_| internal("crate_release_output"))
}

pub(super) fn require_authority_implementation() -> Result<(), OrchestratorError> {
    Err(internal("crate_release_authority_not_qualified"))
}

pub(super) fn poll_pending_admission<T>(
    mut observe: impl FnMut() -> Result<T, OrchestratorError>,
    mut may_retry: impl FnMut() -> bool,
    mut wait: impl FnMut(),
) -> Result<T, OrchestratorError> {
    loop {
        match observe() {
            Err(OrchestratorError::Internal { problem })
                if problem == "crate_release_required_pending" && may_retry() =>
            {
                wait();
            }
            result => return result,
        }
    }
}

pub(super) fn write_checkpoint(
    path: &Path,
    stage: &str,
    receipt: &CrateReleaseAdmissionReceipt,
) -> Result<(), OrchestratorError> {
    let body = serde_json::to_vec(&serde_json::json!({"schema":1,"stage":stage,"receipt":receipt}))
        .map_err(|_| internal("crate_release_checkpoint_write"))?;
    crate::exclusive_write::write_exclusive(path, &body, "crate_release_checkpoint")
}

pub(crate) fn release_mode(receipt: &CrateReleaseAdmissionReceipt) -> &'static str {
    use crate::crate_release_controller::CrateReleaseAction;
    let publish = receipt
        .packages
        .iter()
        .any(|package| matches!(package.action, CrateReleaseAction::Publish));
    let repair = receipt.packages.iter().any(|package| {
        matches!(
            package.action,
            CrateReleaseAction::RepairTag { .. } | CrateReleaseAction::RepairRelease { .. }
        )
    });
    match (publish, repair) {
        (true, true) => "mixed",
        (true, false) => "publish",
        (false, true) => "repair",
        (false, false) => "none",
    }
}
