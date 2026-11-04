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
    if operation == "release-pr-create" {
        return crate::crate_release_pr_run::run_from_environment();
    }
    run_qualified_crate_release(operation)
}

fn run_qualified_crate_release(operation: &str) -> Result<(), OrchestratorError> {
    let workspace = required_path("GITHUB_WORKSPACE")?;
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(
            &required_path("RUNNER_TEMP")?.join("velnor/git-bundle"),
        )
        .map_err(|_| internal("release_git_bundle_invalid"))?,
    );
    run_qualified_crate_release_with_git(operation, &git, &workspace)
}

#[cfg(test)]
fn run_qualified_with_fixture_git(operation: &str) -> Result<(), OrchestratorError> {
    let workspace = required_path("GITHUB_WORKSPACE")?;
    run_qualified_crate_release_with_git(
        operation,
        &crate::crate_release_git::ReleaseGit::host_fixture(),
        &workspace,
    )
}

fn run_qualified_crate_release_with_git(
    operation: &str,
    git: &crate::crate_release_git::ReleaseGit,
    workspace: &Path,
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
            &source,
            operation == "reconciliation-receipt-acquire",
            git,
        );
    }
    if operation == "candidate-receipt" {
        return candidate_receipt(&intent, &request, &github, &source, git);
    }
    if operation == "toolchain-acquire" {
        return acquire_toolchain(&intent, &request, &github, &token, git);
    }
    let registry = NativeCrateRegistryRead;
    verify_runtime_tool_homes(&source)?;
    if is_journal_operation(operation) {
        let candidate = crate::crate_release_github::verify_github_evidence(&request, &github)?;
        crate::crate_release_github::authorize_github_candidate(&request, &candidate, &github)?;
        verify_workflow_concurrency(workspace, &source, &intent, &request, &registry, git)?;
    }
    let receipt_path = required_path("RUNNER_TEMP")?.join("velnor-crate-release-receipt.json");
    dispatch_release_operation(
        operation,
        JournalRuntime {
            receipt_path: &receipt_path,
            intent: &intent,
            request: &request,
            github: &github,
            registry: &registry,
            source: &source,
            token: &token,
            git,
        },
    )
}

fn dispatch_release_operation(
    operation: &str,
    input: JournalRuntime<'_>,
) -> Result<(), OrchestratorError> {
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
        "verify-package" => verify_package(receipt_path, intent, request, github, source, git),
        "admit" => write_admission(receipt_path, intent, request, github, registry, source, git),
        "verify-admission" => {
            verify_admission_output(receipt_path, intent, request, github, registry, source, git)
        }
        "publication-started" => {
            checkpoint_publication(receipt_path, intent, request, github, registry, source, git)
        }
        operation if is_journal_operation(operation) => run_journal(operation, input),
        "reconcile" => reconcile_receipt(receipt_path, intent, request, github, source, git),
        _ => Err(internal("crate_release_operation_invalid")),
    }
}

pub(crate) fn verify_workflow_concurrency<R: crate::crate_release_registry::CrateRegistryRead>(
    workspace: &Path,
    source: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    registry: &R,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let effective_intent =
        crate::crate_release_controller::effective_release_intent(intent, request);
    let plan = crate::crate_release_workspace::crate_release_workspace_plan_with_registry(
        source,
        &request.source_sha,
        &effective_intent,
        registry,
        git,
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
        format!("{workflow_sha}:.github/workflows/release.yml").into(),
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
    if concurrency.len() != 2
        || concurrency.get("group").and_then(serde_json::Value::as_str) != Some(group)
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
    {
        return Err(invalid());
    }
    Ok(())
}

fn is_journal_operation(operation: &str) -> bool {
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

#[derive(Clone, Copy)]
struct JournalRuntime<'a> {
    receipt_path: &'a Path,
    intent: &'a CrateReleaseIntent,
    request: &'a GitHubAdmissionRequest,
    github: &'a NativeGitHubAdmissionRead,
    registry: &'a NativeCrateRegistryRead,
    source: &'a Path,
    token: &'a str,
    git: &'a crate::crate_release_git::ReleaseGit,
}

fn run_journal(operation: &str, input: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    match operation {
        "journal-start" => start_journal(input),
        "journal-coordinate" => coordinate_journal(input),
        "journal-upload-intent"
        | "journal-observe"
        | "journal-repair-tag"
        | "journal-repair-release" => observe_journal_operation(input, operation),
        "journal-recovery-inspect" => inspect_prior_journal(input),
        "journal-reconcile-evidence" => reconcile_journal_evidence(input),
        _ => Err(internal("crate_release_operation_invalid")),
    }
}

struct BootstrapInput {
    manifest: velnor_actions_contract::ReleaseManifest,
    bytes: Vec<u8>,
    manifest_path: PathBuf,
    manifest_bundle: PathBuf,
    binary_path: PathBuf,
    binary_bundle: PathBuf,
}

fn load_bootstrap(
    bootstrap: &Path,
    intent: &CrateReleaseIntent,
) -> Result<BootstrapInput, OrchestratorError> {
    let read = |name: &str, limit| match crate::safe_read::read_repo_bytes(bootstrap, name, limit)?
    {
        crate::safe_read::RepoBytes::Bytes(bytes) if !bytes.is_empty() => Ok(bytes),
        _ => Err(internal("crate_release_bootstrap_file_invalid")),
    };
    let bytes = read("release-manifest.json", 1024 * 1024)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| internal("crate_release_bootstrap_manifest_invalid"))?;
    let manifest = velnor_actions_contract::ReleaseManifest::parse_json_with_limit(
        text,
        "release-manifest.json",
        1024 * 1024,
    )
    .map_err(|_| internal("crate_release_bootstrap_manifest_invalid"))?;
    manifest
        .validate("release-manifest.json")
        .map_err(|_| internal("crate_release_bootstrap_manifest_invalid"))?;
    if manifest.version != intent.generator_version
        || manifest.commit != intent.generator_source_sha
        || crate::generator::sha256_hex(&bytes) != intent.generator_manifest_sha256
        || manifest.repository != "tailrocks/velnor-new"
    {
        return Err(internal("crate_release_bootstrap_manifest_mismatch"));
    }
    let target = manifest
        .record_for_target("x86_64-unknown-linux-gnu")
        .ok_or_else(|| internal("crate_release_bootstrap_target_missing"))?;
    let name = format!(
        "velnor-actions-{}-x86_64-unknown-linux-gnu",
        manifest.version
    );
    let binary = read(&name, 128 * 1024 * 1024)?;
    if crate::generator::sha256_hex(&binary) != target.sha256 {
        return Err(internal("crate_release_bootstrap_binary_mismatch"));
    }
    read("release-manifest.json.intoto.jsonl", 4 * 1024 * 1024)?;
    read(&format!("{name}.intoto.jsonl"), 4 * 1024 * 1024)?;
    Ok(BootstrapInput {
        manifest,
        bytes,
        manifest_path: bootstrap.join("release-manifest.json"),
        manifest_bundle: bootstrap.join("release-manifest.json.intoto.jsonl"),
        binary_path: bootstrap.join(&name),
        binary_bundle: bootstrap.join(format!("{name}.intoto.jsonl")),
    })
}

fn start_journal_with<T>(
    bootstrap: &Path,
    intent: &CrateReleaseIntent,
    reviewed_manifest: &[u8],
    verify: impl FnOnce(&BootstrapInput) -> Result<T, OrchestratorError>,
    record: impl FnOnce(T) -> Result<(), OrchestratorError>,
) -> Result<(), OrchestratorError> {
    let files = load_bootstrap(bootstrap, intent)?;
    if files.bytes != reviewed_manifest {
        return Err(internal(
            "crate_release_bootstrap_reviewed_manifest_mismatch",
        ));
    }
    let evidence = verify(&files)?;
    record(evidence)
}

fn verify_bootstrap_attestation(
    files: &BootstrapInput,
) -> Result<crate::package_release_signer::AttestedLinuxGenerator, OrchestratorError> {
    use crate::package_release_signer::{
        SdkManifestAttestationExpectation, verify_published_linux_binary_attestation,
        verify_published_manifest_attestation,
    };
    let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(180))
        .map_err(|_| internal("crate_release_attestation_deadline"))?;
    let manifest = verify_published_manifest_attestation(
        &files.manifest_path,
        &files.manifest_bundle,
        &SdkManifestAttestationExpectation {
            repository: "tailrocks/velnor-new",
            repository_id: 1_390_620_900,
            owner_id: 78_806_509,
            source_sha: &files.manifest.commit,
            manifest_sha256: &crate::generator::sha256_hex(&files.bytes),
        },
        deadline,
    )?;
    verify_published_linux_binary_attestation(
        &files.binary_path,
        &files.binary_bundle,
        &files.bytes,
        &manifest,
        deadline,
    )
}

fn start_journal(input: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    use crate::crate_release_journal_adapter::JournalAdapter;
    use crate::package_release_signer::{AdmittedStart, record_admitted_start};
    let bootstrap = required_path("RUNNER_TEMP")?.join("velnor/bootstrap");
    let reviewed_manifest = read_manifest_at_workflow(
        &required_path("GITHUB_WORKSPACE")?,
        &input.request.workflow_sha,
        input.git,
    )?;
    start_journal_with(
        &bootstrap,
        input.intent,
        &reviewed_manifest,
        verify_bootstrap_attestation,
        |generator| {
            let admission = read_receipt(input.receipt_path)?;
            let mut observed = prepare_crate_release(
                input.intent,
                input.request,
                input.github,
                input.registry,
                input.source,
                input.git,
            )?;
            observed.semver_evidence = crate::crate_release_semver::verify_preflight(
                input.source,
                &observed,
                input.registry,
                &required_path("RUNNER_TEMP")?,
                input.git,
            )?;
            crate::crate_release_controller::verify_observed_packages(&observed, &admission)?;
            if !crate::crate_release_semver::matches_revalidated_preflight(
                &admission.semver_evidence,
                &observed.semver_evidence,
            ) {
                return Err(internal("crate_release_semver_evidence_changed"));
            }
            let journal =
                JournalAdapter::new(NativeGitHubAdmissionRead::new(input.token.to_owned())?);
            record_admitted_start(&AdmittedStart {
                journal: &journal,
                github: input.github,
                registry: input.registry,
                intent: input.intent,
                request: input.request,
                admission: &admission,
                source: input.source,
                git: input.git,
                workflow_root: &required_path("GITHUB_WORKSPACE")?,
                generator: &generator,
            })?;
            Ok(())
        },
    )
}

fn write_admission(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    registry: &NativeCrateRegistryRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let receipt = prepare_crate_release(intent, request, github, registry, source, git)?;
    let data = serde_json::to_vec(&receipt).map_err(|_| internal("crate_release_receipt_write"))?;
    crate::exclusive_write::write_exclusive(receipt_path, &data, "crate_release_receipt")
}

fn verify_runtime_tool_homes(source: &Path) -> Result<(), OrchestratorError> {
    crate::verify_owned_tool_homes(
        &required_path("RUNNER_TEMP")?,
        &required_path("RUSTUP_HOME")?,
        &required_path("MISE_RUSTUP_HOME")?,
        &required_path("CARGO_HOME")?,
        &required_path("MISE_CARGO_HOME")?,
        source,
    )
}

fn verify_admission_output(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    registry: &NativeCrateRegistryRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let receipt = read_receipt(receipt_path)?;
    verify_crate_release_admission(&receipt, intent, request, github, registry, source, git)?;
    let fresh = prepare_crate_release(intent, request, github, registry, source, git)?;
    crate::crate_release_controller::verify_observed_packages(&fresh, &receipt)?;
    crate::crate_release_github::authorize_github_candidate(request, &receipt.github, github)?;
    append_output(&format!("release_mode={}\n", release_mode(&fresh)))
}

fn reconcile_journal_evidence(input: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::{CrateReleaseObservation, derive_reconciled_candidate};
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    use crate::crate_release_journal_adapter::JournalAdapter;
    let JournalRuntime {
        receipt_path,
        intent,
        request,
        github,
        registry,
        source,
        token,
        git,
    } = input;
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let admission = read_receipt(receipt_path)?;
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: &admission,
        intent,
        request,
        github,
        registry,
        source,
        git,
        sequence: 0,
    };
    let candidate = derive_reconciled_candidate(&input)?;
    let data =
        serde_json::to_vec(&candidate).map_err(|_| internal("crate_release_receipt_write"))?;
    let path = required_path("RUNNER_TEMP")?.join("velnor-reconciled-candidate.json");
    crate::exclusive_write::write_exclusive(&path, &data, "crate_release_candidate")
}

fn inspect_prior_journal(input: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::{CrateReleaseObservation, observe_prior_recovery};
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    use crate::crate_release_journal_adapter::JournalAdapter;
    let JournalRuntime {
        receipt_path,
        intent,
        request,
        github,
        registry,
        source,
        token,
        git,
    } = input;
    let run_text = required("VELNOR_PREVIOUS_RUN_ID")?;
    let run_id = run_text
        .parse::<u64>()
        .map_err(|_| internal("crate_release_previous_invalid"))?;
    let attempt_text = required("VELNOR_PREVIOUS_RUN_ATTEMPT")?;
    let run_attempt = attempt_text
        .parse::<u32>()
        .map_err(|_| internal("crate_release_previous_invalid"))?;
    if run_id == 0
        || run_id.to_string() != run_text
        || run_attempt == 0
        || run_attempt.to_string() != attempt_text
    {
        return Err(internal("crate_release_previous_invalid"));
    }
    let previous = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: required("VELNOR_PREVIOUS_WORKFLOW_SHA")?,
        source_sha: request.source_sha.clone(),
        run_id,
        run_attempt,
    };
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let admission = read_receipt(receipt_path)?;
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: &admission,
        intent,
        request,
        github,
        registry,
        source,
        git,
        sequence: 0,
    };
    let evidence = observe_prior_recovery(&input, &previous)?;
    let data =
        serde_json::to_vec(&evidence).map_err(|_| internal("crate_release_receipt_write"))?;
    let path = required_path("RUNNER_TEMP")?.join("velnor-prior-recovery-evidence.json");
    crate::exclusive_write::write_exclusive(&path, &data, "crate_release_prior_evidence")
}

fn checkpoint_publication(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    registry: &NativeCrateRegistryRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let receipt = read_receipt(receipt_path)?;
    verify_crate_release_admission(&receipt, intent, request, github, registry, source, git)?;
    if release_mode(&receipt) == "none" {
        return Err(internal("crate_release_no_mutation"));
    }
    write_checkpoint(
        &required_path("RUNNER_TEMP")?.join("velnor-crate-release-publication-started.json"),
        "publication-started",
        &receipt,
    )
}

fn coordinate_journal(runtime: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, coordinate_release, invoke_upload,
    };
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    use crate::crate_release_journal_adapter::JournalAdapter;
    use velnor_actions_mise::command::CratePublishCredential;

    let identity = JournalIdentity {
        repository: runtime.request.repository.clone(),
        workflow_sha: runtime.request.workflow_sha.clone(),
        source_sha: runtime.request.source_sha.clone(),
        run_id: runtime.request.run_id,
        run_attempt: runtime.request.run_attempt,
    };
    let admission = read_receipt(runtime.receipt_path)?;
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(runtime.token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: &admission,
        intent: runtime.intent,
        request: runtime.request,
        github: runtime.github,
        registry: runtime.registry,
        source: runtime.source,
        git: runtime.git,
        sequence: 1,
    };
    let runner_temp = required_path("RUNNER_TEMP")?;
    coordinate_release(
        &input,
        |package| {
            crate::crate_release_semver::verify_before_upload(
                runtime.source,
                &admission,
                runtime.registry,
                &runner_temp,
                package,
                runtime.git,
            )
        },
        |permit| {
            let (token, url) = upload_credentials(runtime.intent.authentication)?;
            let credential = match &url {
                Some(request_url) => CratePublishCredential::Trusted {
                    request_url,
                    request_token: &token,
                },
                None => CratePublishCredential::Bootstrap {
                    cargo_token: &token,
                },
            };
            invoke_upload(&input, permit, &runner_temp, runtime.token, credential)
        },
        || thread::sleep(coordinator_wait_duration()),
    )?;
    let reviewed_manifest = read_manifest_at_workflow(
        &required_path("GITHUB_WORKSPACE")?,
        &runtime.request.workflow_sha,
        runtime.git,
    )?;
    start_journal_with(
        &runner_temp.join("velnor/bootstrap"),
        runtime.intent,
        &reviewed_manifest,
        verify_bootstrap_attestation,
        |generator| {
            let tools = crate::package_release_signer::measure_release_tools(
                &runner_temp,
                runtime.intent,
                &started.receipt,
                &generator,
            )?;
            crate::crate_release_journal::record_reconciled_completion(&input, tools)?;
            Ok(())
        },
    )
}

fn coordinator_wait_duration() -> Duration {
    #[cfg(test)]
    if env::var_os("VELNOR_TEST_FAST_POLL").is_some() {
        return Duration::ZERO;
    }
    Duration::from_secs(2)
}

fn upload_credentials(
    authentication: velnor_actions_contract::config::ReleaseAuthentication,
) -> Result<(String, Option<String>), OrchestratorError> {
    use velnor_actions_contract::config::ReleaseAuthentication;
    match authentication {
        ReleaseAuthentication::TrustedPublishing => Ok((
            required("ACTIONS_ID_TOKEN_REQUEST_TOKEN")?,
            Some(required("ACTIONS_ID_TOKEN_REQUEST_URL")?),
        )),
        ReleaseAuthentication::BootstrapToken => Ok((required("CARGO_REGISTRY_TOKEN")?, None)),
    }
}

fn observe_journal_operation(
    runtime: JournalRuntime<'_>,
    operation: &str,
) -> Result<(), OrchestratorError> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, record_verified_mutation, repair_verified_metadata,
        verify_and_record_upload_intent,
    };
    use crate::crate_release_journal::{JournalIdentity, JournalStage, inspect_journal};
    use crate::crate_release_journal_adapter::JournalAdapter;
    let JournalRuntime {
        receipt_path,
        intent,
        request,
        github,
        registry,
        source,
        token,
        git,
    } = runtime;
    let value = required("VELNOR_JOURNAL_SEQUENCE")?;
    let sequence = value
        .parse::<u32>()
        .map_err(|_| internal("crate_release_sequence_invalid"))?;
    if !(1..=1000).contains(&sequence) || sequence.to_string() != value {
        return Err(internal("crate_release_sequence_invalid"));
    }
    let admission = read_receipt(receipt_path)?;
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let journal = JournalAdapter::new(NativeGitHubAdmissionRead::new(token.to_owned())?);
    let started = inspect_journal(&journal, &identity, JournalStage::Started)?
        .ok_or_else(|| internal("crate_release_journal_start_missing"))?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: &admission,
        intent,
        request,
        github,
        registry,
        source,
        git,
        sequence,
    };
    match operation {
        "journal-upload-intent" => {
            let package = required("VELNOR_JOURNAL_PACKAGE")?;
            let semver = crate::crate_release_semver::verify_before_upload(
                source,
                &admission,
                registry,
                &required_path("RUNNER_TEMP")?,
                &package,
                git,
            )?;
            verify_and_record_upload_intent(&input, &package, semver)?;
        }
        "journal-observe" => {
            record_verified_mutation(&input)?;
        }
        "journal-repair-tag" => repair_verified_metadata(
            &input,
            &required("VELNOR_JOURNAL_PACKAGE")?,
            crate::crate_release_journal::JournalMutationKind::CreateTag,
        )?,
        "journal-repair-release" => repair_verified_metadata(
            &input,
            &required("VELNOR_JOURNAL_PACKAGE")?,
            crate::crate_release_journal::JournalMutationKind::CreateRelease,
        )?,
        _ => return Err(internal("crate_release_operation_invalid")),
    }
    Ok(())
}

fn verify_package(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let receipt = read_receipt(receipt_path)?;
    let package = required("VELNOR_JOURNAL_PACKAGE")?;
    let input = crate::crate_release_controller::PackageVerification {
        receipt: &receipt,
        intent,
        request,
        github,
        registry: &NativeCrateRegistryRead,
        source,
        git,
        package: &package,
    };
    crate::crate_release_controller::verify_release_package_with(&input, |observed| {
        crate::crate_release_controller::verify_observed_packages(observed, &receipt)
    })
    .map(|_| ())
}

fn reconcile_receipt(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let receipt = read_receipt(receipt_path)?;
    let completed = reconcile_crate_release(
        &receipt,
        intent,
        request,
        github,
        &NativeCrateRegistryRead,
        source,
        git,
    )?;
    let body = serde_json::to_vec(&serde_json::json!({"schema":1,"stage":"reconciled","receipt":receipt,"packages":completed,"generator_version":env!("CARGO_PKG_VERSION"),"workflow_run":format!("https://github.com/{}/actions/runs/{}", request.repository, request.run_id)})).map_err(|_| internal("crate_release_checkpoint_write"))?;
    crate::exclusive_write::write_exclusive(
        &required_path("RUNNER_TEMP")?.join("velnor-crate-release-reconciled.json"),
        &body,
        "crate_release_checkpoint",
    )
}

pub(crate) fn read_intent_at_workflow(
    workspace: &Path,
    workflow_sha: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<CrateReleaseIntent, OrchestratorError> {
    if !is_full_sha(workflow_sha) {
        return Err(internal("crate_release_workflow_sha"));
    }
    let show = GitRequest::show(vec![
        format!("{workflow_sha}:.github/velnor-release-intent.toml").into(),
    ])
    .run_release_in(workspace, git)
    .map_err(|_| internal("crate_release_intent_read"))?;
    if !show.success || show.stdout.len() > 1_048_576 {
        return Err(internal("crate_release_intent_read"));
    }
    parse_crate_release_intent(&show.stdout)
}

fn read_manifest_at_workflow(
    workspace: &Path,
    workflow_sha: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<Vec<u8>, OrchestratorError> {
    if !is_full_sha(workflow_sha) {
        return Err(internal("crate_release_workflow_sha"));
    }
    let show = GitRequest::show(vec![
        format!("{workflow_sha}:.velnor/release-manifest.json").into(),
    ])
    .run_release_in(workspace, git)
    .map_err(|_| internal("crate_release_reviewed_manifest_read"))?;
    if !show.success || show.stdout.is_empty() || show.stdout.len() > 1024 * 1024 {
        return Err(internal("crate_release_reviewed_manifest_read"));
    }
    Ok(show.stdout)
}

fn candidate_receipt(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let mut receipt = crate::crate_release_controller::prepare_crate_release(
        intent,
        request,
        github,
        &NativeCrateRegistryRead,
        source,
        git,
    )?;
    receipt.semver_evidence = crate::crate_release_semver::verify_preflight(
        source,
        &receipt,
        &NativeCrateRegistryRead,
        &required_path("RUNNER_TEMP")?,
        git,
    )?;
    let data = serde_json::to_vec(&receipt).map_err(|_| internal("crate_release_receipt_write"))?;
    crate::crate_release_tool_artifact::write_receipt(&required_path("RUNNER_TEMP")?, &data)
}

fn is_full_sha(sha: &str) -> bool {
    sha.len() == 40
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn acquire_toolchain(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    token: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let artifact_id = required("VELNOR_RUST_ARTIFACT_ID")?
        .parse()
        .map_err(|_| internal("crate_release_tool_artifact_id"))?;
    let candidate = crate::crate_release_github::verify_github_evidence(request, github)?;
    crate::crate_release_tool_artifact::stage_unverified_toolchain(
        &crate::crate_release_tool_artifact::ToolchainStageRequest {
            intent,
            github: request,
            candidate: &candidate,
            artifact_id,
            token,
            runner_temp: &required_path("RUNNER_TEMP")?,
            git,
            deadline: Instant::now() + Duration::from_mins(15),
        },
        github,
        &crate::crate_release_tool_download::NativeToolDownloadTransport,
    )?;
    Ok(())
}

fn acquire_receipt(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    token: &str,
    source: &std::path::Path,
    reconciliation: bool,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), OrchestratorError> {
    let artifact_id = required("VELNOR_RECEIPT_ARTIFACT_ID")?
        .parse()
        .map_err(|_| internal("crate_release_receipt_artifact_id"))?;
    let candidate = crate::crate_release_github::verify_github_evidence(request, github)?;
    let acquire = if reconciliation {
        crate::crate_release_tool_artifact::acquire_reconciliation_receipt_artifact
    } else {
        crate::crate_release_tool_artifact::acquire_receipt_artifact
    };
    let stage = crate::crate_release_tool_artifact::ToolchainStageRequest {
        intent,
        github: request,
        candidate: &candidate,
        artifact_id,
        token,
        runner_temp: &required_path("RUNNER_TEMP")?,
        git,
        deadline: Instant::now() + Duration::from_mins(5),
    };
    if !reconciliation {
        return acquire(
            &stage,
            github,
            &NativeCrateRegistryRead,
            &crate::crate_release_tool_download::NativeToolDownloadTransport,
            source,
        );
    }
    crate::crate_release_controller::wait_for_reconciliation_visibility(
        12,
        || match acquire(
            &stage,
            github,
            &NativeCrateRegistryRead,
            &crate::crate_release_tool_download::NativeToolDownloadTransport,
            source,
        ) {
            Ok(()) => Ok(true),
            Err(OrchestratorError::Internal { problem })
                if problem == "crate_release_visibility_timeout" =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        },
        || std::thread::sleep(Duration::from_secs(5)),
    )
}

fn request_from_environment(
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

fn source_admit(
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

fn read_receipt(path: &Path) -> Result<CrateReleaseAdmissionReceipt, OrchestratorError> {
    let text = crate::safe_read::read_event_file(path, 1_048_576)
        .map_err(|_| internal("crate_release_receipt_read"))?;
    serde_json::from_str(&text).map_err(|_| internal("crate_release_receipt_invalid"))
}

fn required(name: &str) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("crate_release_missing_{name}")))
}
fn required_path(name: &str) -> Result<PathBuf, OrchestratorError> {
    let path = PathBuf::from(required(name)?);
    if !path.is_absolute() {
        return Err(internal("crate_release_path_invalid"));
    }
    Ok(path)
}
fn append_output(body: &str) -> Result<(), OrchestratorError> {
    let path = required_path("GITHUB_OUTPUT")?;
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|_| internal("crate_release_output"))?;
    file.write_all(body.as_bytes())
        .map_err(|_| internal("crate_release_output"))
}

fn require_authority_implementation() -> Result<(), OrchestratorError> {
    Err(internal("crate_release_authority_not_qualified"))
}

fn poll_pending_admission<T>(
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

fn write_checkpoint(
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

#[cfg(all(test, unix))]
#[path = "crate_release_runtime_rehearsal.rs"]
mod rehearsal;

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn test_only_dispatch_and_loopback_transport() -> Result<(), Box<dyn std::error::Error>> {
        if std::env::var_os("VELNOR_TEST_TRANSPORT_CHILD").is_some() {
            return run_test_transport_child();
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let origin = format!("http://{}", listener.local_addr()?);
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let server_done = std::sync::Arc::clone(&done);
        let server = std::thread::spawn(move || serve_test_transport(&listener, &server_done));
        let (spec, rust) = crate::release_steps::tests::fixture()?;
        let workflow = velnor_actions_workflow_renderer::release_tree::render_release_workflow(
            &spec,
            &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
                generator_version: env!("CARGO_PKG_VERSION").to_owned(),
                runs_on: "ubuntu-24.04".to_owned(),
            },
            &rust,
        )?;
        let workflow: serde_json::Value = serde_saphyr::from_str(&workflow)?;
        let steps = workflow["jobs"]["release-publish"]["steps"]
            .as_array()
            .ok_or("publisher steps")?;
        let script = steps
            .iter()
            .find(|step| step["name"].as_str() == Some("Release journal-start"))
            .and_then(|step| step["run"].as_str())
            .ok_or("journal start script")?;
        let temp = tempfile::tempdir()?;
        let bin = temp.path().join("velnor/bin");
        std::fs::create_dir_all(&bin)?;
        let executable = bin.join(format!("velnor-actions-{}", env!("CARGO_PKG_VERSION")));
        std::fs::write(
            &executable,
            "#!/bin/sh\nexport VELNOR_TEST_COMMAND_ARGS=\"$*\"\nexec \"$VELNOR_TEST_BINARY\" --exact crate_release_runtime::tests::test_only_dispatch_and_loopback_transport\n",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
        }
        let status = std::process::Command::new("bash")
            .args(["-e", "-c", script])
            .env("VELNOR_TEST_TRANSPORT_CHILD", "1")
            .env("VELNOR_TEST_BINARY", std::env::current_exe()?)
            .env("RUNNER_TEMP", temp.path())
            .env("VELNOR_TEST_GITHUB_ORIGIN", &origin)
            .env("VELNOR_TEST_REGISTRY_ORIGIN", &origin)
            .env_remove("GITHUB_WORKSPACE")
            .status()?;
        done.store(true, std::sync::atomic::Ordering::Release);
        server
            .join()
            .map_err(|_| "fixture server panicked")?
            .map_err(|error| format!("fixture server: {error}"))?;
        assert!(status.success());
        Ok(())
    }

    #[cfg(unix)]
    fn run_test_transport_child() -> Result<(), Box<dyn std::error::Error>> {
        use crate::crate_release_controller::CrateReleaseRepairWrite as _;
        use crate::crate_release_github::GitHubAdmissionRead as _;
        use crate::crate_release_journal::JournalIo as _;
        use crate::crate_release_registry::CrateRegistryRead as _;

        assert_eq!(
            std::env::var("VELNOR_TEST_COMMAND_ARGS")?,
            "crate-release journal-start"
        );
        assert!(matches!(
            super::run_crate_release("journal-start"),
            Err(crate::OrchestratorError::Internal { problem })
                if problem == "crate_release_authority_not_qualified"
        ));
        assert!(matches!(
            super::run_qualified_crate_release("journal-start"),
            Err(crate::OrchestratorError::Internal { problem })
                if problem == "crate_release_missing_GITHUB_WORKSPACE"
        ));
        let github = crate::crate_release_github::NativeGitHubAdmissionRead::new("fixture".into())?;
        assert_eq!(github.get("repos/owner/project")?["ok"], true);
        let registry = crate::crate_release_registry::NativeCrateRegistryRead;
        assert_eq!(registry.get("/api/v1/crates/example")?.status, 200);
        let sha = "b".repeat(40);
        github.create_tag("owner/project", "demo-v0.1.1", &sha)?;
        assert_eq!(
            github
                .get_optional("repos/owner/project/git/ref/tags/demo-v0.1.1")?
                .ok_or("missing tag")?["object"]["sha"],
            sha
        );
        github.create_release("owner/project", "demo-v0.1.1", &sha)?;
        assert_eq!(
            github
                .get_optional("repos/owner/project/releases/tags/demo-v0.1.1")?
                .ok_or("missing release")?["id"],
            12
        );
        let journal = crate::crate_release_journal_adapter::JournalAdapter::new(
            crate::crate_release_github::NativeGitHubAdmissionRead::new("fixture".into())?,
        );
        let blob = journal.create_blob("owner/project", b"fixture receipt")?;
        assert_eq!(
            journal.get_blob("owner/project", &blob)?,
            b"fixture receipt"
        );
        let tag = crate::crate_release_journal::JournalTag {
            name: "velnor-receipt/12/1/operation-1".into(),
            object_type: "blob".into(),
            object_sha: blob,
            message: "velnor-release-receipt-v1".into(),
        };
        let tag_sha = journal.create_tag_object("owner/project", &tag)?;
        let reference = format!("refs/tags/{}", tag.name);
        journal.create_ref("owner/project", &reference, &tag_sha)?;
        assert_eq!(
            journal.get_ref("owner/project", &reference)?,
            Some(tag_sha.clone())
        );
        assert!(
            journal
                .create_ref("owner/project", &reference, &tag_sha)
                .is_err()
        );
        assert_eq!(journal.get_ref("owner/project", &reference)?, Some(tag_sha));
        let (owners, presence) =
            crate::crate_release_registry::inspect_crate_registry(&registry, "example", "0.1.1")?;
        assert!(
            matches!(owners, crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } if owners.len() == 1)
        );
        assert_eq!(
            presence,
            crate::crate_release_registry::CrateRegistryPresence::Absent
        );
        verify_test_admissions(&github)?;
        verify_test_journal_refs(&journal)?;
        Ok(())
    }

    #[cfg(unix)]
    fn verify_test_journal_refs(
        journal: &crate::crate_release_journal_adapter::JournalAdapter<
            crate::crate_release_github::NativeGitHubAdmissionRead,
        >,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use crate::crate_release_journal::{JournalIo as _, JournalOperationRefs as _};
        for name in [
            "velnor-receipt/12/1/publication-started",
            "velnor-receipt/12/1/reconciled",
        ] {
            let tag = crate::crate_release_journal::JournalTag {
                name: name.into(),
                object_type: "blob".into(),
                object_sha: journal.create_blob("owner/project", name.as_bytes())?,
                message: "velnor-release-receipt-v1".into(),
            };
            let sha = journal.create_tag_object("owner/project", &tag)?;
            let reference = format!("refs/tags/{name}");
            assert_eq!(journal.get_ref("owner/project", &reference)?, None);
            journal.create_ref("owner/project", &reference, &sha)?;
            assert_eq!(journal.get_ref("owner/project", &reference)?, Some(sha));
        }
        let identity = crate::crate_release_journal::JournalIdentity {
            repository: "owner/project".into(),
            workflow_sha: "b".repeat(40),
            source_sha: "a".repeat(40),
            run_id: 12,
            run_attempt: 1,
        };
        assert_eq!(
            journal
                .list_operation_refs(&identity)?
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["refs/tags/velnor-receipt/12/1/operation-1"]
        );
        Ok(())
    }

    #[cfg(unix)]
    fn verify_test_admissions(
        github: &crate::crate_release_github::NativeGitHubAdmissionRead,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let request = crate::crate_release_github::GitHubAdmissionRequest {
            repository: "owner/project".into(),
            default_branch: "main".into(),
            environment: "release".into(),
            trusted_reviewer_id: 139_017,
            workflow_path: ".github/workflows/release.yml".into(),
            required_workflow_path: ".github/workflows/ci.yml".into(),
            workflow_sha: "b".repeat(40),
            run_id: 12,
            run_attempt: 1,
            event: "workflow_dispatch".into(),
            source_sha: "a".repeat(40),
            recovery_sha: Some("a".repeat(40)),
        };
        let candidate = crate::crate_release_github::verify_github_evidence(&request, github)?;
        crate::crate_release_github::authorize_github_candidate(&request, &candidate, github)?;
        let normal = crate::crate_release_github::GitHubAdmissionRequest {
            run_id: 13,
            event: "push".into(),
            source_sha: request.workflow_sha.clone(),
            recovery_sha: None,
            ..request
        };
        let candidate = crate::crate_release_github::verify_github_evidence(&normal, github)?;
        crate::crate_release_github::authorize_github_candidate(&normal, &candidate, github)?;
        Ok(())
    }

    #[cfg(unix)]
    fn serve_test_transport(
        listener: &std::net::TcpListener,
        done: &std::sync::atomic::AtomicBool,
    ) -> Result<(), String> {
        use std::io::Write as _;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut paths = Vec::new();
        let mut state = TestJournalState::default();
        while !done.load(std::sync::atomic::Ordering::Acquire)
            && std::time::Instant::now() < deadline
        {
            let (mut stream, _) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                Err(error) => return Err(error.to_string()),
            };
            stream.set_nonblocking(false).map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .map_err(|e| e.to_string())?;
            let (line, body) = read_test_request(&mut stream)?;
            let (status, response) = test_service_response(&line, &body, &paths, &mut state)?;
            paths.push(line);
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).map_err(|e| e.to_string())?;
        }
        if paths.is_empty() {
            return Err("no fixture requests".into());
        }
        Ok(())
    }

    #[cfg(unix)]
    pub(super) fn read_test_request(
        stream: &mut std::net::TcpStream,
    ) -> Result<(String, Vec<u8>), String> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let size = stream
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if size == 0 || bytes.len() + size > 65_536 {
                return Err("invalid fixture request".into());
            }
            bytes.extend_from_slice(&buffer[..size]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let header = std::str::from_utf8(&bytes[..end]).map_err(|e| e.to_string())?;
                let length = header
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>())
                    })
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    return Ok((
                        header.lines().next().unwrap_or_default().to_owned(),
                        bytes[end + 4..end + 4 + length].to_vec(),
                    ));
                }
            }
        }
    }

    #[cfg(unix)]
    fn test_service_response(
        line: &str,
        body: &[u8],
        previous: &[String],
        state: &mut TestJournalState,
    ) -> Result<(&'static str, String), String> {
        if line.contains("/git/blobs") {
            return test_blob_response(line, body, state);
        }
        let sha = "b".repeat(40);
        let (tag, release) = test_metadata_responses(&sha);
        let (status, value) = match line {
            "GET /repos/owner/project HTTP/1.1" => (
                "200 OK",
                serde_json::json!({"ok":true,"id":22,"full_name":"owner/project","default_branch":"main"}),
            ),
            "GET /api/v1/crates/example HTTP/1.1" => (
                "200 OK",
                serde_json::json!({"crate":{"id":"example","name":"example","versions":[1]},"versions":[{"id":1,"crate":"example","num":"0.1.0","yanked":false}]}),
            ),
            "GET /api/v1/crates/example/owners HTTP/1.1" => (
                "200 OK",
                serde_json::json!({"users":[{"id":370_532,"login":"owner","kind":"user"}]}),
            ),
            "GET /api/v1/crates/example/0.1.1 HTTP/1.1" => {
                ("404 Not Found", serde_json::json!({"errors":[]}))
            }
            "POST /repos/owner/project/git/refs HTTP/1.1" if previous.len() < 7 => {
                let value: serde_json::Value =
                    serde_json::from_slice(body).map_err(|e| e.to_string())?;
                if value != serde_json::json!({"ref":"refs/tags/demo-v0.1.1","sha":sha}) {
                    return Err("wrong tag mutation".into());
                }
                ("201 Created", tag)
            }
            "GET /repos/owner/project/git/ref/tags/demo-v0.1.1 HTTP/1.1"
                if previous
                    .iter()
                    .any(|entry| entry.starts_with("POST /repos/owner/project/git/refs ")) =>
            {
                ("200 OK", tag)
            }
            "POST /repos/owner/project/releases HTTP/1.1"
                if previous
                    .iter()
                    .any(|entry| entry.starts_with("GET /repos/owner/project/git/ref/tags/")) =>
            {
                let value: serde_json::Value =
                    serde_json::from_slice(body).map_err(|e| e.to_string())?;
                if value
                    != serde_json::json!({"tag_name":"demo-v0.1.1","name":"demo-v0.1.1","target_commitish":sha})
                {
                    return Err("wrong release mutation".into());
                }
                ("201 Created", release)
            }
            "GET /repos/owner/project/releases/tags/demo-v0.1.1 HTTP/1.1"
                if previous
                    .iter()
                    .any(|entry| entry.starts_with("POST /repos/owner/project/releases ")) =>
            {
                ("200 OK", release)
            }
            _ => {
                if let Some(value) =
                    test_github_admission_response(line, &"b".repeat(40), &"a".repeat(40))
                {
                    ("200 OK", value)
                } else {
                    return test_journal_response(line, body, state);
                }
            }
        };
        Ok((status, value.to_string()))
    }

    #[cfg(unix)]
    fn test_blob_response(
        line: &str,
        body: &[u8],
        state: &mut TestJournalState,
    ) -> Result<(&'static str, String), String> {
        use base64::Engine as _;
        let (status, value) = match line {
            "POST /repos/owner/project/git/blobs HTTP/1.1" => {
                let value: serde_json::Value =
                    serde_json::from_slice(body).map_err(|e| e.to_string())?;
                if value["encoding"] != "base64" {
                    return Err("wrong blob encoding".into());
                }
                let content = base64::engine::general_purpose::STANDARD
                    .decode(value["content"].as_str().ok_or("missing blob")?)
                    .map_err(|e| e.to_string())?;
                let sha = crate::release_policy_admission::git_blob_sha1(&content)
                    .map_err(|e| e.to_string())?;
                state.blobs.insert(sha.clone(), content);
                ("201 Created", serde_json::json!({"sha":sha}))
            }
            path if path.starts_with("GET /repos/owner/project/git/blobs/") => {
                let sha = path
                    .strip_prefix("GET /repos/owner/project/git/blobs/")
                    .and_then(|value| value.strip_suffix(" HTTP/1.1"))
                    .ok_or("invalid blob path")?;
                let content = state.blobs.get(sha).ok_or("missing blob")?;
                (
                    "200 OK",
                    serde_json::json!({"sha":sha,"encoding":"base64","size":content.len(),"content":base64::engine::general_purpose::STANDARD.encode(content)}),
                )
            }
            _ => return Err(format!("unexpected blob request: {line}")),
        };
        Ok((status, value.to_string()))
    }

    #[cfg(unix)]
    fn test_metadata_responses(sha: &str) -> (serde_json::Value, serde_json::Value) {
        (
            serde_json::json!({"ref":"refs/tags/demo-v0.1.1","object":{"type":"commit","sha":sha}}),
            serde_json::json!({"id":12,"tag_name":"demo-v0.1.1","draft":false,"html_url":"https://github.com/owner/project/releases/tag/demo-v0.1.1"}),
        )
    }

    #[cfg(unix)]
    pub(super) fn test_github_admission_response(
        line: &str,
        policy: &str,
        source: &str,
    ) -> Option<serde_json::Value> {
        let value = if line == "GET /repos/owner/project/git/ref/heads/main HTTP/1.1" {
            serde_json::json!({"ref":"refs/heads/main","object":{"type":"commit","sha":policy}})
        } else if line.starts_with("GET /repos/owner/project/actions/runs/13/approvals ") {
            serde_json::json!([{"state":"approved","user":{"type":"User","id":139_017},"environments":[{"id":9,"name":"release"}],"comment":format!("I reviewed the complete release policy at {policy} and source {policy}, and authorize this source for run 13.")}])
        } else if line.starts_with("GET /repos/owner/project/actions/runs/13 ") {
            serde_json::json!({"id":13,"run_attempt":1,"head_sha":policy,"head_branch":"main","event":"push","path":".github/workflows/release.yml","repository":{"id":22,"full_name":"owner/project"},"head_repository":{"id":22,"full_name":"owner/project"}})
        } else if line.contains("/pulls?") {
            serde_json::json!([{"number":42,"merged_at":"2026-11-03T16:00:00Z","merge_commit_sha":policy,"base":{"ref":"main","repo":{"full_name":"owner/project"}},"head":{"ref":"release-plz-fixture","sha":"c".repeat(40),"repo":{"full_name":"owner/project"}},"user":{"login":"release-bot"}}])
        } else if line.contains("/git/commits/") {
            let sha = if line.contains(policy) {
                policy
            } else {
                "cccccccccccccccccccccccccccccccccccccccc"
            };
            serde_json::json!({"sha":sha,"tree":{"sha":"d".repeat(40)}})
        } else if line.contains("/pulls/42/reviews?") {
            serde_json::json!([{"id":1,"user":{"id":139_017,"type":"User","login":"owner"},"state":"APPROVED","commit_id":"c".repeat(40),"author_association":"OWNER","submitted_at":"2026-11-03T15:50:00Z"}])
        } else if line.contains("/collaborators/owner/permission") {
            serde_json::json!({"user":{"id":139_017,"type":"User"},"permission":"admin"})
        } else if line.starts_with("GET /repos/owner/project/actions/runs/12/approvals ") {
            serde_json::json!([{"state":"approved","user":{"type":"User","id":139_017},"environments":[{"id":9,"name":"release"}],"comment":format!("I reviewed the complete release policy at {policy} and source {source}, and authorize this source for run 12.")}])
        } else if line.starts_with("GET /repos/owner/project/actions/runs/12 ") {
            serde_json::json!({"id":12,"run_attempt":1,"head_sha":policy,"head_branch":"main","event":"workflow_dispatch","path":".github/workflows/release.yml","repository":{"id":22,"full_name":"owner/project"},"head_repository":{"id":22,"full_name":"owner/project"}})
        } else if line.contains("/compare/") {
            serde_json::json!({"status":"ahead"})
        } else if line.starts_with(
            "GET /repos/owner/project/environments/release/deployment-branch-policies?",
        ) {
            serde_json::json!({"total_count":1,"branch_policies":[{"type":"branch","name":"main"}]})
        } else if line.starts_with("GET /repos/owner/project/environments/release ") {
            serde_json::json!({"id":9,"name":"release","can_admins_bypass":false,"deployment_branch_policy":{"custom_branch_policies":true},"protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"id":139_017}}]}]})
        } else if line.contains("/check-runs?") {
            let sha = if line.contains(policy) {
                policy
            } else {
                source
            };
            serde_json::json!({"total_count":1,"check_runs":[{"id":1,"name":"Required","head_sha":sha,"check_suite":{"id":123},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"}]})
        } else if line.contains("/actions/runs?") {
            let sha = if line.contains(policy) {
                policy
            } else {
                source
            };
            serde_json::json!({"total_count":1,"workflow_runs":[{"check_suite_id":123,"head_sha":sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]})
        } else {
            return None;
        };
        Some(value)
    }

    #[cfg(unix)]
    #[derive(Default)]
    struct TestJournalState {
        blobs: std::collections::BTreeMap<String, Vec<u8>>,
        tags: std::collections::BTreeMap<String, serde_json::Value>,
        refs: std::collections::BTreeMap<String, String>,
    }

    #[cfg(unix)]
    fn test_journal_response(
        line: &str,
        body: &[u8],
        state: &mut TestJournalState,
    ) -> Result<(&'static str, String), String> {
        use sha1_checked::{Digest as _, Sha1};
        let (status, response) = match line {
            path if path.starts_with("GET /repos/owner/project/git/matching-refs/tags/") => {
                ("200 OK", test_matching_refs(path, state)?)
            }
            "POST /repos/owner/project/git/tags HTTP/1.1" => {
                let value: serde_json::Value =
                    serde_json::from_slice(body).map_err(|e| e.to_string())?;
                let blob = value["object"].as_str().ok_or("missing target")?;
                let tag_name = value["tag"].as_str().ok_or("missing tag")?;
                if !tag_name.starts_with("velnor-receipt/")
                    || value["type"] != "blob"
                    || !state.blobs.contains_key(blob)
                {
                    return Err("invalid journal tag".into());
                }
                let raw = format!(
                    "object {blob}\ntype blob\ntag {tag_name}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\nvelnor-release-receipt-v1"
                );
                let mut hash = Sha1::new();
                hash.update(format!("tag {}\0", raw.len()));
                hash.update(raw.as_bytes());
                let digest = hash.try_finalize();
                if digest.has_collision() {
                    return Err("journal hash collision".into());
                }
                let sha = digest
                    .hash()
                    .iter()
                    .fold(String::new(), |mut output, byte| {
                        output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                        output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
                        output
                    });
                let result = serde_json::json!({"sha":sha,"tag":tag_name,"message":"velnor-release-receipt-v1","object":{"type":"blob","sha":blob},"tagger":{"name":"Velnor Release","email":"release@velnor.invalid","date":"1970-01-01T00:00:00Z"},"verification":{"verified":false,"reason":"unsigned","signature":null,"payload":null}});
                state.tags.insert(sha, result.clone());
                ("201 Created", result)
            }
            path if path.starts_with("GET /repos/owner/project/git/tags/") => {
                let sha = path
                    .strip_prefix("GET /repos/owner/project/git/tags/")
                    .and_then(|value| value.strip_suffix(" HTTP/1.1"))
                    .ok_or("invalid tag path")?;
                ("200 OK", state.tags.get(sha).ok_or("missing tag")?.clone())
            }
            "POST /repos/owner/project/git/refs HTTP/1.1" => {
                let value: serde_json::Value =
                    serde_json::from_slice(body).map_err(|e| e.to_string())?;
                let sha = value["sha"].as_str().ok_or("missing tag sha")?;
                let reference = value["ref"].as_str().ok_or("missing ref")?;
                let tag = state.tags.get(sha).ok_or("missing tag")?;
                if tag["tag"]
                    .as_str()
                    .map(|name| format!("refs/tags/{name}"))
                    .as_deref()
                    != Some(reference)
                {
                    return Err("invalid journal ref".into());
                }
                if state.refs.contains_key(reference) {
                    return Ok(("422 Unprocessable Entity", "{}".into()));
                }
                state.refs.insert(reference.to_owned(), sha.into());
                (
                    "201 Created",
                    serde_json::json!({"ref":reference,"object":{"type":"tag","sha":sha}}),
                )
            }
            path if path.starts_with("GET /repos/owner/project/git/ref/tags/velnor-receipt%2F") => {
                return test_journal_ref(path, state);
            }
            _ => return Err(format!("unexpected request: {line}")),
        };
        Ok((status, response.to_string()))
    }

    #[cfg(unix)]
    fn test_journal_ref(
        path: &str,
        state: &TestJournalState,
    ) -> Result<(&'static str, String), String> {
        let name = path
            .strip_prefix("GET /repos/owner/project/git/ref/tags/")
            .and_then(|v| v.strip_suffix(" HTTP/1.1"))
            .ok_or("invalid ref path")?
            .replace("%2F", "/");
        let reference = format!("refs/tags/{name}");
        let Some(sha) = state.refs.get(&reference) else {
            return Ok(("404 Not Found", "{}".into()));
        };
        Ok((
            "200 OK",
            serde_json::json!({"ref":reference,"object":{"type":"tag","sha":sha}}).to_string(),
        ))
    }

    #[cfg(unix)]
    fn test_matching_refs(
        path: &str,
        state: &TestJournalState,
    ) -> Result<serde_json::Value, String> {
        let prefix = path
            .strip_prefix("GET /repos/owner/project/git/matching-refs/tags/")
            .and_then(|v| v.strip_suffix(" HTTP/1.1"))
            .ok_or("invalid prefix")?
            .replace("%2F", "/");
        let prefix = format!("refs/tags/{prefix}");
        let refs: Vec<_> = state
            .refs
            .iter()
            .filter(|(name, _)| name.starts_with(&prefix))
            .map(|(name, sha)| serde_json::json!({"ref":name,"object":{"type":"tag","sha":sha}}))
            .collect();
        Ok(serde_json::json!(refs))
    }

    #[test]
    fn workflow_lock_requires_exact_literal_group_and_no_cancellation() {
        let group = "release-crates-io-owner/repo-workspace";
        let valid = format!(
            "name: release\nconcurrency:\n  group: {group}\n  cancel-in-progress: false\njobs: {{}}\n"
        );
        assert!(super::validate_workflow_concurrency(valid.as_bytes(), group).is_ok());
        for bad in [
            valid.replace(group, "release-crates-io-owner/repo-other"),
            valid.replace("cancel-in-progress: false", "cancel-in-progress: true"),
            valid.replace("concurrency:", "other:"),
            format!("{valid}concurrency:\n  group: {group}\n  cancel-in-progress: false\n"),
            valid.replace(&format!("  group: {group}"), "  group: *other"),
            valid.replace("concurrency:", "concurrency: &value"),
            valid.replace(
                "  cancel-in-progress: false",
                "  cancel-in-progress: false\n  group: other",
            ),
            valid.replace(
                "  cancel-in-progress: false",
                "  cancel-in-progress: false\n  cancel-in-progress: true",
            ),
            valid.replace(
                "  cancel-in-progress: false",
                "  cancel-in-progress: false\n  <<: {}",
            ),
            valid.replace("cancel-in-progress: false", "cancel-in-progress: 'false'"),
            format!("base: &base {{extra: value}}\n<<: *base\n{valid}"),
            valid
                .replace("concurrency:", "base: &base {extra: value}\nconcurrency:")
                .replace(
                    "  cancel-in-progress: false",
                    "  cancel-in-progress: false\n  <<: *base",
                ),
        ] {
            assert!(super::validate_workflow_concurrency(bad.as_bytes(), group).is_err());
        }
    }

    fn reject_unreviewed_bootstrap(
        root: &std::path::Path,
        intent: &velnor_actions_contract::config::CrateReleaseIntent,
        writes: &std::cell::Cell<usize>,
    ) {
        use super::{load_bootstrap, start_journal_with};
        assert!(
            start_journal_with(
                root,
                intent,
                b"different reviewed manifest",
                |_| Ok(()),
                |()| {
                    writes.set(writes.get() + 1);
                    Ok(())
                }
            )
            .is_err()
        );
        assert_eq!(writes.get(), 1);
        let mut wrong = intent.clone();
        wrong.generator_source_sha = "a".repeat(40);
        assert!(load_bootstrap(root, &wrong).is_err());
        wrong = intent.clone();
        wrong.generator_manifest_sha256 = "b".repeat(64);
        assert!(load_bootstrap(root, &wrong).is_err());
    }

    #[test]
    fn journal_start_dispatcher_rejects_tampered_inputs_before_recording()
    -> Result<(), Box<dyn std::error::Error>> {
        use super::{load_bootstrap, start_journal_with};
        use std::{cell::Cell, fs};
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let mut intent: velnor_actions_contract::config::CrateReleaseIntent =
            serde_json::from_value(
                serde_json::json!({"schema":1,"repository":"example/demo","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"rust_version":"1.97.1","generator_version":"0.1.5","generator_source_sha":"f1041f322c54cd7edfdef06afd307799eeaa801a","generator_manifest_sha256":"e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52","release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":null}),
            )?;
        let binary = b"fixture-binary";
        let mut manifest: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../tests/attestation_fixtures/release-manifest.json"
        ))?;
        manifest["targets"][0]["sha256"] = serde_json::json!(crate::generator::sha256_hex(binary));
        let manifest = serde_json::to_vec(&manifest)?;
        intent.generator_manifest_sha256 = crate::generator::sha256_hex(&manifest);
        let name = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
        fs::write(root.join("release-manifest.json"), &manifest)?;
        fs::write(
            root.join("release-manifest.json.intoto.jsonl"),
            b"manifest-signature",
        )?;
        fs::write(root.join(name), binary)?;
        fs::write(
            root.join(format!("{name}.intoto.jsonl")),
            b"binary-signature",
        )?;
        let writes = Cell::new(0);
        let dispatch = || {
            start_journal_with(
                root,
                &intent,
                &manifest,
                |files| {
                    if files.bytes != manifest
                        || fs::read(&files.manifest_bundle).ok().as_deref()
                            != Some(b"manifest-signature")
                        || fs::read(&files.binary_bundle).ok().as_deref()
                            != Some(b"binary-signature")
                    {
                        return Err(crate::internal::internal("fixture_attestation_failed"));
                    }
                    Ok(())
                },
                |()| {
                    writes.set(writes.get() + 1);
                    Ok(())
                },
            )
        };
        dispatch()?;
        assert_eq!(writes.get(), 1);
        reject_unreviewed_bootstrap(root, &intent, &writes);
        fs::write(root.join(name), b"tampered")?;
        assert!(dispatch().is_err());
        fs::write(root.join(name), binary)?;
        fs::write(
            root.join("release-manifest.json"),
            [manifest.as_slice(), b" "].concat(),
        )?;
        assert!(dispatch().is_err());
        fs::write(root.join("release-manifest.json"), &manifest)?;
        fs::write(root.join("release-manifest.json.intoto.jsonl"), b"tampered")?;
        assert!(dispatch().is_err());
        fs::write(
            root.join("release-manifest.json.intoto.jsonl"),
            b"manifest-signature",
        )?;
        fs::write(root.join(format!("{name}.intoto.jsonl")), b"tampered")?;
        assert!(dispatch().is_err());
        assert_eq!(writes.get(), 1);
        assert!(load_bootstrap(root, &intent).is_ok());
        assert!(super::run_crate_release("journal-start").is_err());
        Ok(())
    }
    #[test]
    fn release_intent_comes_from_the_requested_workflow_commit()
    -> Result<(), Box<dyn std::error::Error>> {
        use super::{read_intent_at_workflow, read_manifest_at_workflow};
        use std::{fs, path::Path, process::Command};
        fn test_git() -> crate::crate_release_git::ReleaseGit {
            crate::crate_release_git::ReleaseGit::host_fixture()
        }
        fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
            let output = Command::new("/usr/bin/git")
                .current_dir(root)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()?;
            if !output.status.success() {
                return Err("git fixture failed".into());
            }
            Ok(String::from_utf8(output.stdout)?.trim().to_owned())
        }
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        git(root, &["init", "-q"])?;
        git(root, &["config", "user.name", "Fixture"])?;
        git(root, &["config", "user.email", "fixture@example.invalid"])?;
        git(root, &["config", "commit.gpgsign", "false"])?;
        fs::create_dir(root.join(".github"))?;
        fs::create_dir(root.join(".velnor"))?;
        let path = root.join(".github/velnor-release-intent.toml");
        let manifest_path = root.join(".velnor/release-manifest.json");
        let make = |version: &str| {
            format!(
                "schema=1\nrepository='example/demo'\ndefault_branch='main'\nenvironment='release'\nrequired_workflow_path='.github/workflows/ci.yml'\ntrusted_reviewer_id=139017\nrust_version='1.97.1'\ngenerator_version='{version}'\ngenerator_source_sha='f1041f322c54cd7edfdef06afd307799eeaa801a'\ngenerator_manifest_sha256='e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52'\nrelease_plz_version='0.4.3'\nrunner='ubuntu-24.04'\nauthentication='trusted-publishing'\nmanifest_path='Cargo.toml'\npr_write_paths=['CHANGELOG.md','Cargo.lock','Cargo.toml']\nregistry='crates-io'\npackages=['demo']\npublish_features={{demo=[]}}\ntag_template='{{{{ package }}}}-v{{{{ version }}}}'\n"
            )
        };
        fs::write(&path, make("0.1.5"))?;
        fs::write(&manifest_path, b"first manifest")?;
        git(root, &["add", "."])?;
        git(root, &["commit", "-qm", "first"])?;
        let first = git(root, &["rev-parse", "HEAD"])?;
        fs::write(&path, make("0.1.6"))?;
        fs::write(&manifest_path, b"second manifest")?;
        git(root, &["add", "."])?;
        git(root, &["commit", "-qm", "second"])?;
        let second = git(root, &["rev-parse", "HEAD"])?;
        fs::write(&path, make("0.1.7"))?;
        fs::write(&manifest_path, b"dirty manifest")?;
        assert_eq!(
            read_manifest_at_workflow(root, &first, &test_git())?,
            b"first manifest"
        );
        assert_eq!(
            read_manifest_at_workflow(root, &second, &test_git())?,
            b"second manifest"
        );
        assert!(read_manifest_at_workflow(root, "bad", &test_git()).is_err());
        assert!(read_manifest_at_workflow(root, &"0".repeat(40), &test_git()).is_err());
        assert_eq!(
            read_intent_at_workflow(root, &first, &test_git())?.generator_version,
            "0.1.5"
        );
        assert_eq!(
            read_intent_at_workflow(root, &second, &test_git())?.generator_version,
            "0.1.6"
        );
        assert!(read_intent_at_workflow(root, "bad", &test_git()).is_err());
        assert!(read_intent_at_workflow(root, &"0".repeat(40), &test_git()).is_err());
        Ok(())
    }
    use super::{OrchestratorError, poll_pending_admission};
    use std::cell::Cell;

    #[test]
    fn retries_only_explicitly_pending_required_checks() {
        let calls = Cell::new(0);
        let waits = Cell::new(0);
        let result = poll_pending_admission(
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 3 {
                    Err(OrchestratorError::Internal {
                        problem: "crate_release_required_pending".into(),
                    })
                } else {
                    Ok(7)
                }
            },
            || true,
            || waits.set(waits.get() + 1),
        );
        assert_eq!(result.expect("success"), 7);
        assert_eq!(waits.get(), 2);
        let calls = Cell::new(0);
        let result: Result<(), _> = poll_pending_admission(
            || {
                calls.set(calls.get() + 1);
                Err(OrchestratorError::Internal {
                    problem: "crate_release_check_app".into(),
                })
            },
            || true,
            || panic!("permanent error was retried"),
        );
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
        let result: Result<(), _> = poll_pending_admission(
            || {
                Err(OrchestratorError::Internal {
                    problem: "crate_release_required_pending".into(),
                })
            },
            || false,
            || panic!("expired deadline was retried"),
        );
        assert!(result.is_err());
    }
}
