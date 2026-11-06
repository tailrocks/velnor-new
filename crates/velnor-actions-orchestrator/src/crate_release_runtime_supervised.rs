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
    let custody =
        crate::crate_release_custody::CustodiedChannel::take(absolute).map_err(|_| invalid())?;
    custody.root.revalidate().map_err(|_| invalid())?;
    let token = required("GIT_TOKEN")?;
    let repository = required("GITHUB_REPOSITORY")?;
    let workflow_sha = required("GITHUB_WORKFLOW_SHA")?;
    let git = crate::crate_release_git::ReleaseGit::new(crate::crate_release_git_bundle::acquire(
        custody.root.path(),
        deadline,
    )?);
    let policy = crate::crate_release_checkout::checkout_policy(
        &custody.root,
        &git,
        &repository,
        &workflow_sha,
        &token,
        deadline,
    )?;
    let intent = read_intent_at_workflow(&policy, &workflow_sha, &git)?;
    let request = request_from_environment(&intent, &workflow_sha)?;
    if required("VELNOR_PREFLIGHT_ELIGIBLE")? != "true"
        || required("VELNOR_PREFLIGHT_SOURCE_SHA")? != request.source_sha
    {
        return Err(invalid());
    }
    let github = NativeGitHubAdmissionRead::new(token.clone())?;
    let candidate = verify_github_evidence(&request, &github)?;
    let authority = authorize_github_candidate(&request, &candidate, &github)?;
    if required("GITHUB_REPOSITORY_ID")? != authority.repository_id().to_string()
        || required("GITHUB_REPOSITORY_OWNER_ID")? != authority.owner_id().to_string()
    {
        return Err(invalid());
    }
    let receipt_id = artifact_id("VELNOR_PREFLIGHT_RECEIPT_ARTIFACT_ID")?;
    let rust_id = artifact_id("VELNOR_PREFLIGHT_RUST_ARTIFACT_ID")?;
    if receipt_id == rust_id {
        return Err(invalid());
    }
    let stage = |artifact_id| ToolchainStageRequest {
        intent: &intent,
        github: &request,
        candidate: &candidate,
        artifact_id,
        token: &token,
        runner_temp: custody.root.path(),
        deadline: instant,
    };
    let transport = crate::crate_release_tool_download::NativeToolDownloadTransport;
    let receipt = crate::crate_release_tool_artifact::receipt::acquire_authenticated_receipt(
        &stage(receipt_id),
        &github,
        &transport,
    )?;
    if receipt.authority() != &authority {
        return Err(invalid());
    }
    let source = crate::crate_release_checkout::checkout_authorized_source(
        &custody.view,
        &authority,
        &git,
        &token,
        deadline,
    )?;
    let snapshot = AuthenticatedSource::snapshot(receipt, &source, &git, deadline)?;
    crate::crate_release_tool_artifact::receipt::write_receipt(
        custody.root.path(),
        snapshot.receipt().bytes(),
    )?;
    let staged = crate::crate_release_tool_artifact::stage_unverified_toolchain(
        &stage(rust_id),
        &github,
        &transport,
    )?;
    crate::crate_release_pinned_download::acquire(&custody.root, &intent, deadline)?;
    let projected = crate::crate_release_projected_tools::ProjectedToolchain::project(
        &custody.root,
        &custody.view,
        staged,
        &authority,
        &intent,
    )?;
    let sealed = crate::crate_release_source_view::SealedSourceView::seal(
        snapshot,
        custody.view,
        projected,
        git.clone(),
        &github,
        deadline,
    )?;
    let execution = sealed
        .project_source_tools(&custody.root)?
        .connect(custody.socket, absolute)?;
    let registry = NativeCrateRegistryRead;
    verify_workflow_concurrency(
        &policy,
        execution.root(),
        &intent,
        &request,
        &registry,
        &git,
        &execution,
    )?;
    let receipt_path = custody
        .root
        .path()
        .join("velnor-crate-release-receipt.json");
    let runtime = JournalRuntime {
        receipt_path: &receipt_path,
        workflow_root: &policy,
        runner_temp: custody.root.path(),
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: execution.root(),
        token: &token,
        git: &git,
        upload: super::journal_start::UploadRuntime::Supervised(&execution),
    };
    super::journal_start::start_journal(runtime)?;
    let frozen = super::journal_coordinate::prepare_coordinate_journal(runtime)?;
    let closed = execution.finish(frozen)?;
    super::journal_coordinate::complete_journal(closed, &token, &github, &registry)
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
