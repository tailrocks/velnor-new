// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_controller::{
    CrateReleaseAdmissionReceipt, prepare_crate_release, reconcile_crate_release,
    verify_crate_release_admission,
};
use crate::crate_release_github::{
    GitHubAdmissionOutcome, GitHubAdmissionRequest, NativeGitHubAdmissionRead,
    admit_github_evidence,
};
use crate::crate_release_registry::NativeCrateRegistryRead;
use crate::{OrchestratorError, internal::internal};
use std::{
    env,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_mise::GitRequest;

pub fn parse_crate_release_intent(bytes: &[u8]) -> Result<CrateReleaseIntent, OrchestratorError> {
    if bytes.len() > velnor_actions_contract::config::MAX_CRATE_RELEASE_INTENT_BYTES {
        return Err(internal("crate_release_intent_parse"));
    }
    let intent: CrateReleaseIntent = toml::from_str(
        std::str::from_utf8(bytes).map_err(|_| internal("crate_release_intent_parse"))?,
    )
    .map_err(|_| internal("crate_release_intent_parse"))?;
    intent
        .validate()
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    Ok(intent)
}

pub fn run_crate_release(operation: &str) -> Result<(), OrchestratorError> {
    require_authority_implementation()?;
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    if operation == "publish-supervise" {
        return supervised::run();
    }
    if operation == "release-pr-create" {
        return crate::release::crate_release_pr_run::run_from_environment();
    }
    run_qualified_crate_release(operation)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "crate_release_runtime_supervised.rs"]
mod supervised;

fn run_qualified_crate_release(operation: &str) -> Result<(), OrchestratorError> {
    let workspace = required_path("GITHUB_WORKSPACE")?;
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(
            &required_path("RUNNER_TEMP")?.join("velnor/git-bundle"),
        )
        .map_err(|_| internal("release_git_bundle_invalid"))?,
    );
    run_qualified_crate_release_with_git(
        operation,
        &git,
        &workspace,
        &crate::UnprivilegedSourceExecution,
    )
}

#[cfg(test)]
fn run_qualified_with_fixture_git(operation: &str) -> Result<(), OrchestratorError> {
    let workspace = required_path("GITHUB_WORKSPACE")?;
    run_qualified_crate_release_with_git(
        operation,
        &crate::crate_release_git::ReleaseGit::host_fixture(),
        &workspace,
        &crate::UnprivilegedSourceExecution,
    )
}

fn run_qualified_crate_release_with_git(
    operation: &str,
    git: &crate::crate_release_git::ReleaseGit,
    workspace: &Path,
    source_execution: &crate::UnprivilegedSourceExecution,
) -> Result<(), OrchestratorError> {
    let workflow_sha = required("GITHUB_SHA")?;
    let intent = read_intent_at_workflow(workspace, &workflow_sha, git)?;
    let request = request_from_environment(&intent, &workflow_sha)?;
    let token = required("GIT_TOKEN")?;
    if matches!(operation, "source-admit" | "source-require") {
        return source_admit(&request, &token, operation == "source-require");
    }
    let github = NativeGitHubAdmissionRead::new(token.clone())?;
    let source = workspace.join("release-source");
    if matches!(
        operation,
        "receipt-acquire" | "reconciliation-receipt-acquire"
    ) {
        return acquire_receipt(
            &intent,
            &request,
            &github,
            &token,
            crate::CrateSource::new(&source, git, source_execution),
            operation == "reconciliation-receipt-acquire",
        );
    }
    if operation == "candidate-receipt" {
        return candidate_receipt(&intent, &request, &github, &source, git, source_execution);
    }
    if operation == "verify-preflight-retention" {
        let receipt_id = required("VELNOR_RECEIPT_ARTIFACT_ID")?
            .parse()
            .map_err(|_| internal("crate_release_retention_receipt_id"))?;
        let rust_id = required("VELNOR_RUST_ARTIFACT_ID")?
            .parse()
            .map_err(|_| internal("crate_release_retention_rust_id"))?;
        crate::crate_release_github::verify_github_evidence(&request, &github)?;
        return crate::crate_release_tool_artifact::verify_preflight_retention_now(
            &request, &github, receipt_id, rust_id,
        );
    }
    if operation == "toolchain-acquire" {
        return acquire_toolchain(&intent, &request, &github, &token);
    }
    let registry = NativeCrateRegistryRead;
    verify_runtime_tool_homes(&source)?;
    if is_journal_operation(operation) {
        let candidate = crate::crate_release_github::verify_github_evidence(&request, &github)?;
        crate::crate_release_github::authorize_github_candidate(&request, &candidate, &github)?;
        verify_workflow_concurrency(
            workspace,
            &source,
            &intent,
            &request,
            &registry,
            git,
            source_execution,
        )?;
    }
    let runner_temp = required_path("RUNNER_TEMP")?;
    let receipt_path = runner_temp.join("velnor-crate-release-receipt.json");
    dispatch_release_operation(
        operation,
        JournalRuntime {
            receipt_path: &receipt_path,
            workflow_root: workspace,
            runner_temp: &runner_temp,
            intent: &intent,
            request: &request,
            github: &github,
            registry: &registry,
            source: &source,
            token: &token,
            git,
            upload: journal_start::UploadRuntime::Unprivileged(source_execution),
        },
    )
}

fn dispatch_release_operation(
    operation: &str,
    input: JournalRuntime<'_>,
) -> Result<(), OrchestratorError> {
    let source_execution = input.upload.source_execution();
    let JournalRuntime {
        receipt_path,
        intent,
        request,
        github,
        registry,
        source,
        git,
        ..
    } = input;
    match operation {
        "verify-package" => verify_package(
            receipt_path,
            intent,
            request,
            github,
            source,
            git,
            source_execution,
        ),
        "admit" => write_admission(
            receipt_path,
            intent,
            request,
            github,
            registry,
            crate::CrateSource::new(source, git, source_execution),
        ),
        "verify-admission" => verify_admission_output(
            receipt_path,
            intent,
            request,
            github,
            registry,
            crate::CrateSource::new(source, git, source_execution),
        ),
        "publication-started" => checkpoint_publication(
            receipt_path,
            intent,
            request,
            github,
            registry,
            crate::CrateSource::new(source, git, source_execution),
        ),
        operation if is_journal_operation(operation) => run_journal(operation, input),
        "reconcile" => reconcile_receipt(
            receipt_path,
            intent,
            request,
            github,
            source,
            git,
            source_execution,
        ),
        _ => Err(internal("crate_release_operation_invalid")),
    }
}

#[path = "crate_release_runtime_workflow.rs"]
mod workflow;
use workflow::is_journal_operation;

#[path = "crate_release_runtime_journal_start.rs"]
mod journal_start;
use journal_start::{
    JournalRuntime, run_journal, start_journal_with, verify_admission_output,
    verify_bootstrap_attestation, verify_runtime_tool_homes, write_admission,
};

#[path = "crate_release_runtime_journal_reconcile.rs"]
mod journal_reconcile;
use journal_reconcile::{
    checkpoint_publication, inspect_prior_journal, reconcile_journal_evidence,
};

#[path = "crate_release_runtime_journal_coordinate.rs"]
mod journal_coordinate;
use journal_coordinate::coordinate_journal;

#[path = "crate_release_runtime_journal_observe.rs"]
mod journal_observe;
use journal_observe::{observe_journal_operation, reconcile_receipt, verify_package};

#[path = "crate_release_runtime_admission.rs"]
mod admission;
use admission::{
    acquire_receipt, acquire_toolchain, candidate_receipt, is_full_sha, read_manifest_at_workflow,
};

#[path = "crate_release_runtime_environment.rs"]
mod environment;
#[cfg(test)]
pub(crate) use environment::request_from_environment as test_request_from_environment;
use environment::{
    append_output, read_receipt, request_from_environment, require_authority_implementation,
    required, required_path, source_admit, write_checkpoint,
};

pub(crate) use admission::read_intent_at_workflow;
pub(crate) use environment::release_mode;
pub(crate) use workflow::verify_workflow_concurrency;
#[cfg(test)]
pub(crate) use workflow::{read_workflow_at, validate_workflow_concurrency};

#[cfg(test)]
use environment::poll_pending_admission;
#[cfg(test)]
use journal_start::load_bootstrap;

#[cfg(all(test, unix))]
#[path = "crate_release_runtime_rehearsal.rs"]
mod rehearsal;

#[cfg(test)]
#[path = "crate_release_runtime_tests.rs"]
mod tests;
