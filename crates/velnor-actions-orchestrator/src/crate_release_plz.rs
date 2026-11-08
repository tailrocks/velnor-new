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
use super::{CrateReleaseAction, observe_admitted_crate_release};
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
    let cargo_home = create_cargo_home(private.path())?;
    let cargo_target = create_private_target(private.path())?;
    let args = upload_args(private.path(), input.intent, &permit.package, &manifest)?;
    let provider_path = prepare_provider(private.path())?;
    let provider = provider_path.to_str().ok_or_else(invalid)?;
    let socket = private.path().join("authorization.sock");
    let env: Vec<(OsString, OsString)> = [
        (
            "PATH",
            format!(
                "{}:/usr/bin:/bin",
                cargo.parent().ok_or_else(invalid)?.display()
            ),
        ),
        ("CARGO", cargo.display().to_string()),
        ("RUSTC", rustc.display().to_string()),
        ("RUSTDOC", rustdoc.display().to_string()),
        ("HOME", private.path().display().to_string()),
        ("CARGO_HOME", cargo_home.display().to_string()),
        ("CARGO_TARGET_DIR", cargo_target.display().to_string()),
        ("RUSTUP_HOME", rustup_home.display().to_string()),
        ("RUSTUP_AUTO_INSTALL", "0".to_owned()),
        ("RUNNER_TEMP", runner_temp.display().to_string()),
        (
            "CARGO_REGISTRIES_CRATES_IO_CREDENTIAL_PROVIDER",
            provider.to_owned(),
        ),
        ("CARGO_REGISTRY_CREDENTIAL_PROVIDER", provider.to_owned()),
        ("CARGO_REGISTRY_DEFAULT", "crates-io".to_owned()),
        ("VELNOR_CARGO_AUTH_SOCKET", socket.display().to_string()),
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
    let attempt = if provider::execute_upload(&command, &git_root, &socket, input, &permit)? {
        UploadAttempt::ExitedSuccess
    } else {
        UploadAttempt::Uncertain
    };
    if crate::verify_crate_release_source(input.source, &permit.source_sha, input.git).is_err() {
        return Ok(UploadAttempt::Uncertain);
    }
    Ok(attempt)
}

fn create_cargo_home(private: &Path) -> Result<std::path::PathBuf, OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    let home = private.join("cargo");
    std::fs::create_dir(&home).map_err(|_| invalid())?;
    #[cfg(all(test, unix))]
    if let Some(snapshot) = std::env::var_os("VELNOR_NATIVE_REGISTRY_SNAPSHOT") {
        crate::release::crate_release_semver::test_registry::copy_snapshot(
            Path::new(&snapshot),
            &home,
        )
        .map_err(|_| invalid())?;
    }
    Ok(home)
}

fn upload_args(
    private: &Path,
    intent: &CrateReleaseIntent,
    package: &str,
    manifest: &Path,
) -> Result<Vec<OsString>, OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    let config_path = private.join("release-plz.toml");
    std::fs::write(&config_path, single_package_config(intent, package)?).map_err(|_| invalid())?;
    Ok(vec![
        "release".into(),
        "--config".into(),
        config_path.into_os_string(),
        "--manifest-path".into(),
        manifest.as_os_str().to_owned(),
    ])
}

#[cfg(unix)]
fn prepare_provider(private: &Path) -> Result<std::path::PathBuf, OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    #[cfg(test)]
    let executable = std::env::var_os("VELNOR_NATIVE_PROVIDER_EXECUTABLE")
        .map(std::path::PathBuf::from)
        .map_or_else(std::env::current_exe, Ok)
        .map_err(|_| invalid())?;
    #[cfg(not(test))]
    let executable = std::env::current_exe().map_err(|_| invalid())?;
    let provider = private.join("credential-provider");
    std::os::unix::fs::symlink(executable, &provider).map_err(|_| invalid())?;
    provider
        .to_str()
        .filter(|value| !value.chars().any(char::is_whitespace))
        .ok_or_else(invalid)?;
    Ok(provider)
}

#[cfg(not(unix))]
fn prepare_provider(_: &Path) -> Result<std::path::PathBuf, OrchestratorError> {
    Err(internal("crate_release_upload_unverified"))
}

fn create_private_target(home: &Path) -> Result<std::path::PathBuf, OrchestratorError> {
    let target = home.join("target");
    std::fs::create_dir(&target).map_err(|_| internal("crate_release_upload_unverified"))?;
    Ok(target)
}

fn upload_rust_tools(
    home: &Path,
    version: &str,
) -> Result<[std::path::PathBuf; 3], OrchestratorError> {
    let tool =
        |name| crate::release::crate_release_source::verified_rust_tool_path(home, version, name);
    Ok([tool("cargo")?, tool("rustc")?, tool("rustdoc")?])
}

#[path = "crate_release_plz_provider.rs"]
mod provider;

pub(crate) fn verify_current_upload_intent<J: JournalIo>(
    journal: &J,
    identity: &crate::crate_release_journal::JournalIdentity,
    receipt: &crate::crate_release_journal::JournalReceipt,
    sequence: u32,
    package: &str,
    version: &str,
    checksum: &str,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    let started = inspect_journal(journal, identity, JournalStage::Started)?.ok_or_else(invalid)?;
    if started.receipt != *receipt
        || inspect_journal(journal, identity, JournalStage::Completed)?.is_some()
    {
        return Err(invalid());
    }
    let item = inspect_journal_mutation(journal, identity, sequence)?.ok_or_else(invalid)?;
    if item.kind != JournalMutationKind::Upload
        || item.phase != JournalMutationPhase::Intended
        || item.package != package
        || item.version != version
        || item.archive_sha256.as_deref() != Some(checksum)
        || inspect_journal_mutation(
            journal,
            identity,
            sequence.checked_add(1).ok_or_else(invalid)?,
        )?
        .is_some()
    {
        return Err(invalid());
    }
    Ok(())
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
    if crate::planner::generator::sha256_hex(&bytes) != expected {
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
    if !matches!(
        (credential, input.intent.authentication),
        (
            CratePublishCredential::Trusted { .. },
            ReleaseAuthentication::TrustedPublishing
        ) | (
            CratePublishCredential::Bootstrap { .. },
            ReleaseAuthentication::BootstrapToken
        )
    ) {
        return Err(invalid());
    }
    validate_upload_permit(input, permit)
}

pub(super) fn validate_upload_permit<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    permit: &UploadPermit,
) -> Result<velnor_actions_mise::catalog::QualifiedReleasePlz, OrchestratorError> {
    let invalid = || internal("crate_release_upload_unverified");
    input.intent.validate().map_err(|_| invalid())?;
    verify_binding(input)?;
    let bytes =
        velnor_actions_contract::canonical_json_bytes(input.intent).map_err(|_| invalid())?;
    if permit.intent_sha256 != crate::planner::generator::sha256_hex(&bytes)
        || permit.repository != input.request.repository
        || permit.workflow_sha != input.request.workflow_sha
        || permit.source_sha != input.request.source_sha
        || permit.run_id != input.request.run_id
        || permit.run_attempt != input.request.run_attempt
        || permit.intention_ref != mutation_name(input.identity, permit.sequence)
        || !input.intent.packages.contains(&permit.package)
    {
        return Err(invalid());
    }
    verify_current_upload_intent(
        input.journal,
        input.identity,
        input.journal_receipt,
        permit.sequence,
        &permit.package,
        &permit.version,
        &permit.archive_sha256,
    )?;
    let current = observe_admitted_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        crate::CrateSource::new(input.source, input.git, input.source_execution),
        &input.admission.github,
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
    if input.journal_receipt.expected_tools.release_plz_sha256 != qualified.executable_sha256 {
        return Err(invalid());
    }
    Ok(qualified)
}

pub(crate) fn single_package_config(
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
                version_group: intent
                    .version_groups
                    .iter()
                    .find_map(|(group, members)| members.contains(name).then(|| group.clone())),
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
