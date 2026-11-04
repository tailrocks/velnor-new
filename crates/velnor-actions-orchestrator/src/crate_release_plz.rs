// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{ffi::OsString, path::Path, time::Duration};
use velnor_actions_contract::config::{CrateReleaseIntent, ReleaseAuthentication};
use velnor_actions_mise::command::{CratePublishCredential, IsolatedCommand};
use velnor_actions_workflow_renderer::release_config::{
    ReleasePlzConfig, ReleasePlzPackage, render_single_package_upload_config,
};

use super::observation::{
    CrateReleaseObservation, UploadAttempt, UploadPermit, verify_binding,
    verify_observed_packages_after_recorded_upload,
};
use super::{CrateReleaseAction, observe_crate_release};
use crate::crate_release_github::{GitHubAdmissionRead, authorize_github_candidate};
use crate::crate_release_journal::{
    JournalIo, JournalMutationKind, JournalMutationPhase, JournalStage, inspect_journal,
    inspect_journal_mutation, mutation_name,
};
use crate::crate_release_registry::CrateRegistryRead;
use crate::{OrchestratorError, internal::internal};

#[expect(
    clippy::needless_pass_by_value,
    reason = "the upload permit is consumed exactly once"
)]
pub(crate) fn invoke_upload<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    permit: UploadPermit,
    runner_temp: &Path,
    git_token: &str,
    credential: CratePublishCredential<'_>,
) -> Result<UploadAttempt, OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    let qualified = validate_upload(input, &permit, &credential)?;
    let runner_temp = runner_temp.canonicalize().map_err(|_| invalid())?;
    let executable = verified_release_plz(&runner_temp, qualified.executable_sha256)?;
    let git_root = runner_temp.join("velnor/git-bundle");
    let git =
        velnor_actions_mise::release_git_pin::verify_bundle(&git_root).map_err(|_| invalid())?;
    let rustup_home = runner_temp.join("velnor/rustup");
    let [cargo, rustc, rustdoc] = upload_rust_tools(&rustup_home, &input.intent.rust_version)?;
    let bin = cargo.parent().ok_or_else(invalid)?;
    let source = input.source.canonicalize().map_err(|_| invalid())?;
    if runner_temp.starts_with(&source) {
        return Err(invalid());
    }
    let manifest = source
        .join(&input.intent.manifest_path)
        .canonicalize()
        .map_err(|_| invalid())?;
    if !manifest.starts_with(&source) || !manifest.is_file() {
        return Err(invalid());
    }
    let private = tempfile::Builder::new()
        .prefix("velnor-upload-")
        .tempdir_in(&runner_temp)
        .map_err(|_| invalid())?;
    let cargo_home = private.path().join("cargo");
    std::fs::create_dir(&cargo_home).map_err(|_| invalid())?;
    let config_path = private.path().join("release-plz.toml");
    let config = single_package_config(input.intent, &permit.package)?;
    std::fs::write(&config_path, config).map_err(|_| invalid())?;
    let args = vec![
        "release".into(),
        "--config".into(),
        config_path.into_os_string(),
        "--manifest-path".into(),
        manifest.into_os_string(),
    ];
    let env: Vec<(OsString, OsString)> = [
        ("PATH", format!("{}:/usr/bin:/bin", bin.display())),
        ("CARGO", cargo.display().to_string()),
        ("RUSTC", rustc.display().to_string()),
        ("RUSTDOC", rustdoc.display().to_string()),
        ("HOME", private.path().display().to_string()),
        ("CARGO_HOME", cargo_home.display().to_string()),
        ("RUSTUP_HOME", rustup_home.display().to_string()),
        ("RUSTUP_AUTO_INSTALL", "0".to_owned()),
        ("RUNNER_TEMP", runner_temp.display().to_string()),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    let command = IsolatedCommand::crate_publish(
        &executable,
        args,
        &source,
        &env,
        git_token,
        credential,
        &git,
    )
    .map_err(|_| invalid())?;
    let attempt = if execute_upload(&command, &git_root)? {
        UploadAttempt::ExitedSuccess
    } else {
        UploadAttempt::Uncertain
    };
    if crate::verify_crate_release_source(input.source, &permit.source_sha, input.git).is_err() {
        return Ok(UploadAttempt::Uncertain);
    }
    Ok(attempt)
}

fn upload_rust_tools(
    home: &Path,
    version: &str,
) -> Result<[std::path::PathBuf; 3], OrchestratorError> {
    let tool = |name| crate::crate_release_source::verified_rust_tool_path(home, version, name);
    Ok([tool("cargo")?, tool("rustc")?, tool("rustdoc")?])
}

fn execute_upload(command: &IsolatedCommand, git_root: &Path) -> Result<bool, OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(600))
        .map_err(|_| invalid())?;
    velnor_actions_mise::release_git_pin::verify_bundle(git_root).map_err(|_| invalid())?;
    let result = command.run_quiescent_until(1024 * 1024, deadline);
    velnor_actions_mise::release_git_pin::verify_bundle(git_root).map_err(|_| invalid())?;
    let (output, _drained) = result.map_err(|_| internal("crate_release_upload_incomplete"))?;
    Ok(output.success)
}

fn verified_release_plz(
    runner_temp: &Path,
    expected: &str,
) -> Result<std::path::PathBuf, OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    let relative = "velnor-release-plz/release-plz";
    let bytes = match crate::safe_read::read_repo_bytes(runner_temp, relative, 128 * 1024 * 1024)? {
        crate::safe_read::RepoBytes::Bytes(bytes) => bytes,
        crate::safe_read::RepoBytes::Absent => return Err(invalid()),
    };
    if crate::generator::sha256_hex(&bytes) != expected {
        return Err(invalid());
    }
    Ok(runner_temp.join(relative))
}

pub(super) fn validate_upload<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    permit: &UploadPermit,
    credential: &CratePublishCredential<'_>,
) -> Result<velnor_actions_mise::catalog::QualifiedReleasePlz, OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    input.intent.validate().map_err(|_| invalid())?;
    verify_binding(input)?;
    let bytes =
        velnor_actions_contract::canonical_json_bytes(input.intent).map_err(|_| invalid())?;
    if permit.intent_sha256 != crate::generator::sha256_hex(&bytes)
        || permit.repository != input.request.repository
        || permit.workflow_sha != input.request.workflow_sha
        || permit.source_sha != input.request.source_sha
        || permit.run_id != input.request.run_id
        || permit.run_attempt != input.request.run_attempt
        || permit.intention_ref != mutation_name(input.identity, permit.sequence)
        || !input.intent.packages.contains(&permit.package)
        || !matches!(
            (credential, input.intent.authentication),
            (
                CratePublishCredential::Trusted { .. },
                ReleaseAuthentication::TrustedPublishing
            ) | (
                CratePublishCredential::Bootstrap { .. },
                ReleaseAuthentication::BootstrapToken
            )
        )
    {
        return Err(invalid());
    }
    let started = inspect_journal(input.journal, input.identity, JournalStage::Started)?
        .ok_or_else(invalid)?;
    if started.receipt != *input.journal_receipt {
        return Err(invalid());
    }
    let mutation = inspect_journal_mutation(input.journal, input.identity, permit.sequence)?
        .ok_or_else(invalid)?;
    if mutation.sequence != permit.sequence
        || mutation.package != permit.package
        || mutation.version != permit.version
        || mutation.kind != JournalMutationKind::Upload
        || mutation.phase != JournalMutationPhase::Intended
    {
        return Err(invalid());
    }
    let current = observe_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
        input.git,
    )?;
    let next_sequence = permit.sequence.checked_add(1).ok_or_else(invalid)?;
    verify_observed_packages_after_recorded_upload(
        &CrateReleaseObservation {
            sequence: next_sequence,
            ..*input
        },
        &current,
    )?;
    if !current.packages.iter().any(|item| {
        item.source.name == permit.package
            && item.source.version == permit.version
            && item.action == CrateReleaseAction::Publish
    }) {
        return Err(invalid());
    }
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    crate::verify_crate_release_source(input.source, &permit.source_sha, input.git)?;
    let qualified = velnor_actions_mise::catalog::qualified_release_plz(
        &input.intent.release_plz_version,
        &input.intent.runner,
    )
    .ok_or_else(invalid)?;
    if started.receipt.expected_tools.release_plz_sha256 != qualified.executable_sha256 {
        return Err(invalid());
    }
    Ok(qualified)
}

pub(super) fn single_package_config(
    intent: &CrateReleaseIntent,
    package: &str,
) -> Result<String, OrchestratorError> {
    let packages = intent
        .packages
        .iter()
        .map(|name| {
            let publish_features = intent
                .publish_features
                .get(name)
                .cloned()
                .ok_or_else(|| internal("crate_release_upload_unverified"))?;
            Ok(ReleasePlzPackage {
                name: name.clone(),
                publish_features,
            })
        })
        .collect::<Result<Vec<_>, OrchestratorError>>()?;
    let config = ReleasePlzConfig {
        tag_pattern: intent.tag_template.clone(),
        semver_check: true,
        packages,
    };
    render_single_package_upload_config(&config, package, &intent.generator_version)
        .map_err(|_| internal("crate_release_upload_unverified"))
}
