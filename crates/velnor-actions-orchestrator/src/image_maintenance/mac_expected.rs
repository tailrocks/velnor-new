// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OrchestratorError, internal::internal, package_release_publish::read_protected_source_files,
};
use velnor_actions_contract::{
    config::VelnorConfig,
    runner_image_manifest::mac_attestation::{MAC_COMPANION_RUNNER_ID, MacSdkAuthority, MacSdkIdentity},
};
use velnor_actions_mise::{CheckDeadline, catalog::lock::parse_generator_lock};

const REPOSITORY: &str = "tailrocks/velnor-new";
const REPOSITORY_ID: u64 = 1_390_620_900;
const MAC_TARGET: &str = "aarch64-apple-darwin";

pub(super) struct ExpectedMacSource {
    pub(super) workflow_sha: String,
    pub(super) sdk: MacSdkIdentity,
    pub(super) generator_lock: velnor_actions_contract::GeneratorLock,
    config: VelnorConfig,
    workflow: Vec<u8>,
}

pub(super) fn load_source(
    source: &str,
    run: u64,
    attempt: u32,
    token: &str,
    runner_temp: &std::path::Path,
    q_execution: bool,
    deadline: CheckDeadline,
) -> Result<ExpectedMacSource, OrchestratorError> {
    let (config, lock, workflow) = load_workflow_source(source, deadline)?;
    let mac = config.runner_image_family.as_ref().and_then(|family| family.mac_qualification.as_ref()).ok_or_else(|| internal("mac_authority_source_invalid"))?;
    let sdk = sdk_identity(mac, &lock, &SdkRequest { source, run, attempt, token, runner_temp, q_execution, deadline })?;
    Ok(ExpectedMacSource { workflow_sha: mac.companion_workflow_sha.clone(), sdk, generator_lock: lock, config, workflow })
}

pub(super) fn load_workflow_source(source: &str, deadline: CheckDeadline) -> Result<(VelnorConfig, velnor_actions_contract::GeneratorLock, Vec<u8>), OrchestratorError> {
    let invalid = || internal("mac_authority_source_invalid");
    let mut files = read_protected_source_files(
        REPOSITORY,
        REPOSITORY_ID,
        "main",
        source,
        &[
            ".velnor/config.toml",
            ".velnor/generator.lock",
            ".github/workflows/product-release-images.yml",
        ],
        1024 * 1024,
        deadline,
    )?;
    let config = files.remove(".velnor/config.toml").ok_or_else(invalid)?;
    let config: VelnorConfig = toml::from_str(std::str::from_utf8(&config).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    config
        .validate(".velnor/config.toml")
        .map_err(|_| invalid())?;
    let mac = config
        .runner_image_family
        .as_ref()
        .and_then(|family| family.mac_qualification.as_ref())
        .ok_or_else(invalid)?;
    if mac.runner_id != MAC_COMPANION_RUNNER_ID
        || mac.companion_workflow_path != ".github/workflows/qualify-runner-family.yml"
        || mac.companion_workflow_ref != "refs/heads/main"
    {
        return Err(invalid());
    }
    let bytes = files.remove(".velnor/generator.lock").ok_or_else(invalid)?;
    let lock = parse_generator_lock(std::str::from_utf8(&bytes).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let workflow = files.remove(".github/workflows/product-release-images.yml").ok_or_else(invalid)?;
    Ok((config, lock, workflow))
}

struct SdkRequest<'a> {
    source: &'a str,
    run: u64,
    attempt: u32,
    token: &'a str,
    runner_temp: &'a std::path::Path,
    q_execution: bool,
    deadline: CheckDeadline,
}

fn sdk_identity(
    mac: &velnor_actions_contract::config::MacRunnerQualificationConfig,
    lock: &velnor_actions_contract::GeneratorLock,
    request: &SdkRequest<'_>,
) -> Result<MacSdkIdentity, OrchestratorError> {
    let invalid = || internal("mac_authority_source_invalid");
    match (&mac.sdk, request.q_execution) {
        (
            velnor_actions_contract::config::MacSdkSelection::Stable {
                version,
                source_sha,
                manifest_sha256,
            },
            false,
        ) => {
            if lock.generator.binary != "velnor-actions"
                || lock.generator.version != *version
                || lock.generator.commit != *source_sha
            {
                return Err(invalid());
            }
            let asset = lock.binary_for_target(MAC_TARGET).ok_or_else(invalid)?;
            Ok(MacSdkIdentity {
                repository: REPOSITORY.to_owned(),
                version: version.clone(),
                source_commit: source_sha.clone(),
                authority: MacSdkAuthority::Stable {
                    release_manifest_sha256: manifest_sha256.clone(),
                },
                target: MAC_TARGET.to_owned(),
                asset_sha256: asset.sha256.clone(),
            })
        }
        (velnor_actions_contract::config::MacSdkSelection::SourcePreview {}, true) => {
            let gh = std::env::var_os("VELNOR_Q_GH").ok_or_else(invalid)?;
            super::mac_preview_sdk::verify(&super::mac_preview_sdk::VerifyRequest {
                source: request.source,
                run: request.run,
                attempt: request.attempt,
                lock,
                gh: std::path::Path::new(&gh),
                temp: request.runner_temp,
                token: request.token,
                deadline: request.deadline,
            })
        }
        _ => Err(invalid()),
    }
}

impl ExpectedMacSource {
    pub(super) fn verify_workflow(&self) -> Result<(), OrchestratorError> {
        verify_workflow(&self.config, &self.sdk.version, &self.workflow)
    }
}

pub(super) fn verify_workflow(config: &VelnorConfig, version: &str, workflow: &[u8]) -> Result<(), OrchestratorError> {
        let files = crate::routing::extra_files(
            config,
            &velnor_actions_mise::ToolCatalog::pinned(),
            version,
        )?;
        let mut matches = files
            .iter()
            .filter(|file| file.path == ".github/workflows/product-release-images.yml");
        let file = matches
            .next()
            .ok_or_else(|| internal("mac_authority_workflow_missing"))?;
        if matches.next().is_some() || file.bytes.as_bytes() != workflow {
            return Err(internal("mac_authority_workflow_mismatch"));
        }
        Ok(())
    }
