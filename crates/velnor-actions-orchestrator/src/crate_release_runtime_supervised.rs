// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Duration, JournalRuntime, NativeCrateRegistryRead, NativeGitHubAdmissionRead,
    OrchestratorError, internal, read_intent_at_workflow, request_from_environment, required,
    verify_workflow_concurrency,
};
use crate::crate_release_github::{authorize_github_candidate, verify_github_evidence};
use crate::crate_release_tool_artifact::{ToolchainStageRequest, receipt::AuthenticatedSource};
use velnor_actions_mise::CheckDeadline;

pub(super) fn run() -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_supervisor_invalid");
    let (absolute, deadline, instant) = operation_deadline()?;
    let custody =
        crate::crate_release_custody::CustodiedChannel::take(absolute).map_err(|_| invalid())?;
    custody.root.revalidate().map_err(|_| invalid())?;
    let admission = admit_workflow(&custody.root, deadline)?;
    let (receipt_id, rust_id) = preflight_artifact_ids()?;
    deadline
        .scope(|| {
            crate::crate_release_tool_artifact::verify_preflight_retention_now(
                &admission.request,
                &admission.github,
                receipt_id,
                rust_id,
            )
        })
        .map_err(|_| invalid())??;
    let stage = |artifact_id| ToolchainStageRequest {
        intent: &admission.intent,
        github: &admission.request,
        candidate: &admission.candidate,
        artifact_id,
        token: &admission.token,
        runner_temp: custody.root.path(),
        deadline: instant,
    };
    let transport = crate::crate_release_tool_download::NativeToolDownloadTransport;
    let snapshot = acquire_source_snapshot(
        &stage(receipt_id),
        &admission.github,
        &transport,
        &admission.authority,
        &custody.view,
        &admission.git,
        deadline,
    )?;
    let staged = crate::crate_release_tool_artifact::stage_unverified_toolchain(
        &stage(rust_id),
        &admission.github,
        &transport,
    )?;
    crate::crate_release_pinned_download::acquire(&custody.root, &admission.intent, deadline)?;
    let projected = crate::crate_release_projected_tools::ProjectedToolchain::project(
        &custody.root,
        &custody.view,
        staged,
        &admission.authority,
        &admission.intent,
    )?;
    let sealed = crate::crate_release_source_view::SealedSourceView::seal(
        snapshot,
        custody.view,
        projected,
        admission.git.clone(),
        &admission.github,
        deadline,
    )?;
    let execution = sealed
        .project_source_tools(&custody.root)?
        .connect(custody.socket, absolute)?;
    let registry = NativeCrateRegistryRead;
    let frozen = prepare_journal(&admission, &custody.root, &execution, &registry)?;
    let closed = execution.finish(frozen)?;
    super::journal_coordinate::complete_journal(
        closed,
        &admission.token,
        &admission.github,
        &registry,
    )
}

fn prepare_journal(
    admission: &WorkflowAdmission,
    root: &crate::crate_release_custody::PublisherRoot,
    execution: &crate::crate_release_source_view::SupervisedSourceExecution,
    registry: &NativeCrateRegistryRead,
) -> Result<crate::crate_release_controller::FrozenCompletion, OrchestratorError> {
    verify_workflow_concurrency(
        &admission.policy,
        execution.root(),
        &admission.intent,
        &admission.request,
        registry,
        &admission.git,
        execution,
    )?;
    let receipt_path = root.path().join("velnor-crate-release-receipt.json");
    let runtime = JournalRuntime {
        receipt_path: &receipt_path,
        workflow_root: &admission.policy,
        runner_temp: root.path(),
        intent: &admission.intent,
        request: &admission.request,
        github: &admission.github,
        registry,
        source: execution.root(),
        token: &admission.token,
        git: &admission.git,
        upload: super::journal_start::UploadRuntime::Supervised(execution),
    };
    super::journal_start::start_journal(runtime)?;
    super::journal_coordinate::prepare_coordinate_journal(runtime)
}

struct WorkflowAdmission {
    token: String,
    git: crate::crate_release_git::ReleaseGit,
    policy: std::path::PathBuf,
    intent: velnor_actions_contract::config::CrateReleaseIntent,
    request: crate::crate_release_github::GitHubAdmissionRequest,
    github: NativeGitHubAdmissionRead,
    candidate: crate::crate_release_github::GitHubCandidate,
    authority: crate::crate_release_github::GitHubAuthorized,
}

fn admit_workflow(
    root: &crate::crate_release_custody::PublisherRoot,
    deadline: CheckDeadline,
) -> Result<WorkflowAdmission, OrchestratorError> {
    let token = required("GIT_TOKEN")?;
    let repository = required("GITHUB_REPOSITORY")?;
    let workflow_sha = required("GITHUB_WORKFLOW_SHA")?;
    let git = crate::crate_release_git::ReleaseGit::new(crate::crate_release_git_bundle::acquire(
        root.path(),
        deadline,
    )?);
    let policy = crate::crate_release_checkout::checkout_policy(
        root,
        &git,
        &repository,
        &workflow_sha,
        &token,
        deadline,
    )?;
    let intent = read_intent_at_workflow(&policy, &workflow_sha, &git)?;
    let request = request_from_environment(&intent, &workflow_sha)?;
    check_preflight(&request)?;
    let github = NativeGitHubAdmissionRead::new(token.clone())?;
    let candidate = verify_github_evidence(&request, &github)?;
    let authority = authorize_github_candidate(&request, &candidate, &github)?;
    check_repository_identity(&authority)?;
    Ok(WorkflowAdmission {
        token,
        git,
        policy,
        intent,
        request,
        github,
        candidate,
        authority,
    })
}

fn check_preflight(
    request: &crate::crate_release_github::GitHubAdmissionRequest,
) -> Result<(), OrchestratorError> {
    if required("VELNOR_PREFLIGHT_ELIGIBLE")? != "true"
        || required("VELNOR_PREFLIGHT_SOURCE_SHA")? != request.source_sha
    {
        return Err(internal("crate_release_supervisor_invalid"));
    }
    Ok(())
}

fn check_repository_identity(
    authority: &crate::crate_release_github::GitHubAuthorized,
) -> Result<(), OrchestratorError> {
    if required("GITHUB_REPOSITORY_ID")? != authority.repository_id().to_string()
        || required("GITHUB_REPOSITORY_OWNER_ID")? != authority.owner_id().to_string()
    {
        return Err(internal("crate_release_supervisor_invalid"));
    }
    Ok(())
}

fn acquire_source_snapshot(
    stage: &ToolchainStageRequest<'_>,
    github: &NativeGitHubAdmissionRead,
    transport: &crate::crate_release_tool_download::NativeToolDownloadTransport,
    authority: &crate::crate_release_github::GitHubAuthorized,
    view: &crate::crate_release_custody::PublisherView,
    git: &crate::crate_release_git::ReleaseGit,
    deadline: CheckDeadline,
) -> Result<AuthenticatedSource, OrchestratorError> {
    let invalid = || internal("crate_release_supervisor_invalid");
    let receipt = crate::crate_release_tool_artifact::receipt::acquire_authenticated_receipt(
        stage, github, transport,
    )?;
    if receipt.authority() != authority {
        return Err(invalid());
    }
    let source = crate::crate_release_checkout::checkout_authorized_source(
        view,
        authority,
        git,
        stage.token,
        deadline,
    )?;
    let snapshot = AuthenticatedSource::snapshot(receipt, &source, git, deadline)?;
    crate::crate_release_tool_artifact::receipt::write_receipt(
        stage.runner_temp,
        snapshot.receipt().bytes(),
    )?;
    Ok(snapshot)
}

fn operation_deadline() -> Result<(u64, CheckDeadline, std::time::Instant), OrchestratorError> {
    let invalid = || internal("crate_release_supervisor_invalid");
    let absolute: u64 = required("VELNOR_OPERATION_DEADLINE_BOOTTIME_NS")?
        .parse()
        .map_err(|_| invalid())?;
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Boottime);
    let now = u64::try_from(clock.tv_sec)
        .map_err(|_| invalid())?
        .checked_mul(1_000_000_000)
        .and_then(|v| v.checked_add(u64::try_from(clock.tv_nsec).ok()?))
        .ok_or_else(invalid)?;
    let remaining = Duration::from_nanos(
        absolute
            .checked_sub(now)
            .filter(|v| *v > 0)
            .ok_or_else(invalid)?,
    );
    let deadline = CheckDeadline::after(remaining).map_err(|_| invalid())?;
    let instant = std::time::Instant::now()
        .checked_add(remaining)
        .ok_or_else(invalid)?;
    Ok((absolute, deadline, instant))
}

fn preflight_artifact_ids() -> Result<(u64, u64), OrchestratorError> {
    let receipt_id = artifact_id("VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID")?;
    let rust_id = artifact_id("VELNOR_PREFLIGHT_RUST_ARTIFACT_ID")?;
    if receipt_id == rust_id {
        return Err(internal("crate_release_supervisor_invalid"));
    }
    Ok((receipt_id, rust_id))
}

fn artifact_id(key: &str) -> Result<u64, OrchestratorError> {
    let value = required(key)?;
    let parsed: u64 = value
        .parse()
        .map_err(|_| internal("crate_release_artifact_invalid"))?;
    if parsed == 0 || parsed.to_string() != value {
        return Err(internal("crate_release_artifact_invalid"));
    }
    Ok(parsed)
}
