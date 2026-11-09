// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    mac_expected, mac_inbox,
    mac_proof::{VerifyMacQualificationRequest, verify_mac_qualification},
    publication::github,
};
use crate::{OrchestratorError, internal::internal};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    env,
    io::{Cursor, Read},
    path::PathBuf,
};
use velnor_actions_contract::runner_image_manifest::{
    RunnerImageManifest, mac_attestation::MacPublicAuthority,
};
use velnor_actions_mise::CheckDeadline;

#[derive(Debug)]
pub struct VerifyMacAttestationRequest {
    pub producer_run_id: u64,
    pub producer_run_attempt: u32,
    pub producer_artifact_id: u64,
    pub publisher_artifact_id: u64,
    pub assets_dir: PathBuf,
    pub runner_temp: PathBuf,
    pub deadline: CheckDeadline,
}

#[derive(Debug)]
pub struct VerifiedMacAttestation {
    pub manifest_sha256: String,
    pub checksums_sha256: String,
    pub license_sha256: String,
    pub notice_sha256: String,
}

struct VerifiedAssets {
    manifest: Vec<u8>,
    digests: [String; 4],
}

#[derive(Deserialize)]
struct Artifact {
    id: u64,
    name: String,
    expired: bool,
    size_in_bytes: u64,
    digest: Option<String>,
    workflow_run: ArtifactRun,
}
#[derive(Deserialize)]
struct ArtifactRun {
    id: u64,
    head_sha: String,
    head_branch: String,
}

pub fn verify_mac_attestation(
    request: &VerifyMacAttestationRequest,
) -> Result<VerifiedMacAttestation, OrchestratorError> {
    let invalid = || internal("mac_release_authority_invalid");
    let Invocation {
        token,
        source,
        run,
        attempt,
        execution,
    } = invocation()?;
    if request.producer_artifact_id == 0
        || request.publisher_artifact_id == 0
        || !request.assets_dir.is_absolute()
        || !request.runner_temp.is_absolute()
    {
        return Err(invalid());
    }
    authenticate(request, &source, run, attempt, &token, execution)?;
    let expected = mac_expected::load_source(
        &source,
        run,
        attempt,
        &token,
        &request.runner_temp,
        execution.is_some(),
        request.deadline,
    )?;
    if execution.is_none() {
        crate::homebrew::sdk_binding::verify_sdk_for_mac(
            &expected.sdk,
            &expected.generator_lock,
            request.deadline,
        )?;
    }
    expected.verify_workflow()?;
    let verified_assets = verify_publisher_artifact(request, &source, run, attempt, &token)?;
    let manifest_bytes = &verified_assets.manifest;
    let manifest_value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(manifest_bytes, 1024 * 1024)
            .map_err(|_| invalid())?;
    let manifest: RunnerImageManifest =
        serde_json::from_value(manifest_value).map_err(|_| invalid())?;
    let public = MacPublicAuthority {
        repository: "tailrocks/velnor-new".to_owned(),
        repository_id: 1_390_620_900,
        source_commit: source.clone(),
        release_run_id: run,
        release_run_attempt: attempt,
        producer_artifact_id: request.producer_artifact_id,
        publisher_artifact_id: request.publisher_artifact_id,
    };
    let proof = mac_inbox::discover(
        run,
        attempt,
        &hex(&Sha256::digest(manifest_bytes)),
        &token,
        request.deadline,
    )?;
    verify_mac_qualification(&VerifyMacQualificationRequest {
        envelope: &proof.envelope,
        bundle: &proof.bundle,
        public: &public,
        sdk: &expected.sdk,
        manifest_bytes,
        manifest: &manifest,
        workflow_sha: &expected.workflow_sha,
        runner_temp: &request.runner_temp,
        deadline: request.deadline,
    })?;
    authenticate(request, &source, run, attempt, &token, execution)?;
    let [
        manifest_sha256,
        checksums_sha256,
        license_sha256,
        notice_sha256,
    ] = verified_assets.digests;
    Ok(VerifiedMacAttestation {
        manifest_sha256,
        checksums_sha256,
        license_sha256,
        notice_sha256,
    })
}

struct Invocation {
    token: String,
    source: String,
    run: u64,
    attempt: u32,
    execution: Option<(&'static str, u64)>,
}

fn invocation() -> Result<Invocation, OrchestratorError> {
    let invalid = || internal("mac_release_authority_invalid");
    let token = env::var("GH_TOKEN").map_err(|_| invalid())?;
    let source = env::var("GITHUB_SHA").map_err(|_| invalid())?;
    let run: u64 = env::var("GITHUB_RUN_ID")
        .map_err(|_| invalid())?
        .parse()
        .map_err(|_| invalid())?;
    let attempt: u32 = env::var("GITHUB_RUN_ATTEMPT")
        .map_err(|_| invalid())?
        .parse()
        .map_err(|_| invalid())?;
    let execution = execution_authority()?;
    if token.is_empty() || run == 0 || attempt == 0 {
        return Err(invalid());
    }
    Ok(Invocation {
        token,
        source,
        run,
        attempt,
        execution,
    })
}

fn execution_authority() -> Result<Option<(&'static str, u64)>, OrchestratorError> {
    let invalid = || internal("mac_release_authority_invalid");
    let job = env::var("GITHUB_JOB").map_err(|_| invalid())?;
    if job == "verify-mac-proof" {
        return Ok(None);
    }
    let sha = velnor_actions_contract::source_preview::RUNTIME_AUTHORITY_SHA.ok_or_else(invalid)?;
    let path = ".github/workflows/source-preview-mac-proof.yml";
    if job != "verify"
        || env::var("VELNOR_Q_REPOSITORY").map_err(|_| invalid())? != "tailrocks/velnor-new"
        || env::var("VELNOR_Q_PATH").map_err(|_| invalid())? != path
        || env::var("VELNOR_Q_SHA").map_err(|_| invalid())? != sha
        || env::var("VELNOR_Q_REF").map_err(|_| invalid())?
            != format!("tailrocks/velnor-new/{path}@{sha}")
    {
        return Err(invalid());
    }
    let raw = env::var("VELNOR_Q_CHECK_RUN_ID").map_err(|_| invalid())?;
    let id: u64 = raw.parse().map_err(|_| invalid())?;
    if id == 0 || id.to_string() != raw {
        return Err(invalid());
    }
    Ok(Some((sha, id)))
}

fn authenticate(
    request: &VerifyMacAttestationRequest,
    source: &str,
    run: u64,
    attempt: u32,
    token: &str,
    execution: Option<(&str, u64)>,
) -> Result<(), OrchestratorError> {
    let deadline = request.deadline;
    github::authenticate_runner_invocation(source, request.producer_run_id, token, deadline)?;
    if github::ci::main_sha(token, deadline)? != source {
        return Err(internal("mac_release_main_changed"));
    }
    let ci = github::ci::authenticate_source(source, token, deadline)?;
    if ci.id != request.producer_run_id || ci.attempt != request.producer_run_attempt {
        return Err(internal("mac_release_producer_mismatch"));
    }
    github::authenticate_runner_producer_artifact(
        source,
        ci.id,
        ci.attempt,
        request.producer_artifact_id,
        token,
        deadline,
    )?;
    github::authenticate_release_jobs(source, run, attempt, token, deadline, execution)
}

fn verify_publisher_artifact(
    request: &VerifyMacAttestationRequest,
    source: &str,
    run: u64,
    attempt: u32,
    token: &str,
) -> Result<VerifiedAssets, OrchestratorError> {
    let invalid = || internal("mac_release_publisher_artifact_invalid");
    let route = format!(
        "repos/tailrocks/velnor-new/actions/artifacts/{}",
        request.publisher_artifact_id
    );
    let artifact: Artifact = github::read::get_json(&route, token, request.deadline)?;
    if artifact.id != request.publisher_artifact_id
        || artifact.name != format!("runner-image-release-{attempt}")
        || artifact.expired
        || artifact.size_in_bytes == 0
        || artifact.size_in_bytes > 16 * 1024 * 1024
        || artifact.workflow_run.id != run
        || artifact.workflow_run.head_sha != source
        || artifact.workflow_run.head_branch != "main"
    {
        return Err(invalid());
    }
    let digest = artifact.digest.ok_or_else(invalid)?;
    let archive = github::read::get_bytes(&format!("{route}/zip"), token, request.deadline)?;
    if archive.len() as u64 != artifact.size_in_bytes
        || digest != format!("sha256:{}", hex(&Sha256::digest(&archive)))
    {
        return Err(invalid());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|_| invalid())?;
    if zip.len() != 4 {
        return Err(invalid());
    }
    let mut manifest = None;
    let mut digests = Vec::new();
    for name in [
        "RUNNER_IMAGE_MANIFEST.json",
        "SHA256SUMS",
        "LICENSE",
        "NOTICE",
    ] {
        let mut entry = zip.by_name(name).map_err(|_| invalid())?;
        if !entry.is_file()
            || entry.size() > 1024 * 1024
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
        {
            return Err(invalid());
        }
        let declared = entry.size();
        let mut bytes = Vec::new();
        (&mut entry)
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        if bytes.len() as u64 != declared || bytes.len() > 1024 * 1024 {
            return Err(invalid());
        }
        let local = super::validate::read_limited(&request.assets_dir.join(name), 1024 * 1024)?;
        if bytes != local {
            return Err(invalid());
        }
        digests.push(format!("sha256:{}", hex(&Sha256::digest(&bytes))));
        if name == "RUNNER_IMAGE_MANIFEST.json" {
            manifest = Some(bytes);
        }
    }
    Ok(VerifiedAssets {
        manifest: manifest.ok_or_else(invalid)?,
        digests: digests.try_into().map_err(|_| invalid())?,
    })
}

fn hex(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|v| {
            [
                char::from(ALPHABET[usize::from(v >> 4)]),
                char::from(ALPHABET[usize::from(v & 15)]),
            ]
        })
        .collect()
}
