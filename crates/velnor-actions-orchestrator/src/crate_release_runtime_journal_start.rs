// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseIntent, Duration, GitHubAdmissionRequest, NativeCrateRegistryRead,
    NativeGitHubAdmissionRead, OrchestratorError, Path, PathBuf, append_output, coordinate_journal,
    inspect_prior_journal, internal, observe_journal_operation, prepare_crate_release,
    read_manifest_at_workflow, read_receipt, reconcile_journal_evidence, release_mode,
    required_path, verify_crate_release_admission,
};

#[derive(Clone, Copy)]
pub(crate) enum UploadRuntime<'a> {
    Unprivileged(&'a crate::UnprivilegedSourceExecution),
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    Supervised(&'a crate::crate_release_source_view::SupervisedSourceExecution),
}

impl<'a> UploadRuntime<'a> {
    pub(super) fn source_execution(self) -> &'a dyn crate::CrateSourceExecution {
        match self {
            Self::Unprivileged(execution) => execution,
            #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
            Self::Supervised(execution) => execution,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct JournalRuntime<'a> {
    pub(super) receipt_path: &'a Path,
    pub(super) workflow_root: &'a Path,
    pub(super) runner_temp: &'a Path,
    pub(super) intent: &'a CrateReleaseIntent,
    pub(super) request: &'a GitHubAdmissionRequest,
    pub(super) github: &'a NativeGitHubAdmissionRead,
    pub(super) registry: &'a NativeCrateRegistryRead,
    pub(super) source: &'a Path,
    pub(super) token: &'a str,
    pub(super) git: &'a crate::crate_release_git::ReleaseGit,
    pub(super) upload: UploadRuntime<'a>,
}

pub(super) fn run_journal(
    operation: &str,
    input: JournalRuntime<'_>,
) -> Result<(), OrchestratorError> {
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

pub(super) struct BootstrapInput {
    pub(super) manifest: velnor_actions_contract::ReleaseManifest,
    pub(super) bytes: Vec<u8>,
    pub(super) manifest_path: PathBuf,
    pub(super) manifest_bundle: PathBuf,
    pub(super) binary_path: PathBuf,
    pub(super) binary_bundle: PathBuf,
}

pub(super) fn load_bootstrap(
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

pub(super) fn start_journal_with<T>(
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

pub(super) fn verify_bootstrap_attestation(
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

pub(super) fn start_journal(input: JournalRuntime<'_>) -> Result<(), OrchestratorError> {
    use crate::crate_release_journal_adapter::JournalAdapter;
    use crate::package_release_signer::{AdmittedStart, record_admitted_start};
    let source_execution = input.upload.source_execution();
    let bootstrap = input.runner_temp.join("velnor/bootstrap");
    let reviewed_manifest =
        read_manifest_at_workflow(input.workflow_root, &input.request.workflow_sha, input.git)?;
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
                crate::CrateSource::new(input.source, input.git, source_execution),
            )?;
            observed.semver_evidence = crate::crate_release_semver::verify_preflight(
                input.source,
                &observed,
                input.registry,
                input.runner_temp,
                input.git,
                source_execution,
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
                source_execution,
                journal: &journal,
                github: input.github,
                registry: input.registry,
                intent: input.intent,
                request: input.request,
                admission: &admission,
                source: input.source,
                git: input.git,
                workflow_root: input.workflow_root,
                generator: &generator,
            })?;
            Ok(())
        },
    )
}

pub(super) fn write_admission(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    registry: &NativeCrateRegistryRead,
    source_context: crate::CrateSource<'_>,
) -> Result<(), OrchestratorError> {
    let crate::CrateSource {
        root: source,
        git,
        execution: source_execution,
    } = source_context;
    let receipt = prepare_crate_release(
        intent,
        request,
        github,
        registry,
        crate::CrateSource::new(source, git, source_execution),
    )?;
    let data = serde_json::to_vec(&receipt).map_err(|_| internal("crate_release_receipt_write"))?;
    velnor_actions_contract::exclusive_write::write_exclusive(
        receipt_path,
        &data,
        "crate_release_receipt",
    )
    .map_err(Into::into)
}

pub(super) fn verify_runtime_tool_homes(source: &Path) -> Result<(), OrchestratorError> {
    crate::verify_owned_tool_homes(
        &required_path("RUNNER_TEMP")?,
        &required_path("RUSTUP_HOME")?,
        &required_path("MISE_RUSTUP_HOME")?,
        &required_path("CARGO_HOME")?,
        &required_path("MISE_CARGO_HOME")?,
        source,
    )
}

pub(super) fn verify_admission_output(
    receipt_path: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &NativeGitHubAdmissionRead,
    registry: &NativeCrateRegistryRead,
    source_context: crate::CrateSource<'_>,
) -> Result<(), OrchestratorError> {
    let crate::CrateSource {
        root: source,
        git,
        execution: source_execution,
    } = source_context;
    let receipt = read_receipt(receipt_path)?;
    verify_crate_release_admission(
        &receipt,
        intent,
        request,
        github,
        registry,
        crate::CrateSource::new(source, git, source_execution),
    )?;
    let fresh = prepare_crate_release(
        intent,
        request,
        github,
        registry,
        crate::CrateSource::new(source, git, source_execution),
    )?;
    crate::crate_release_controller::verify_observed_packages(&fresh, &receipt)?;
    crate::crate_release_github::authorize_github_candidate(request, &receipt.github, github)?;
    append_output(&format!("release_mode={}\n", release_mode(&fresh)))
}
