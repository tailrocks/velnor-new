// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{mac_expected, mac_signer, publication::github};
use crate::{OrchestratorError, internal::internal};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::{Cursor, Read}, path::PathBuf};
use velnor_actions_mise::CheckDeadline;

#[derive(Debug)]
pub struct AdmitMacPublicationRequest {
    pub source_commit: String,
    pub release_run_id: u64,
    pub release_run_attempt: u32,
    pub assets_dir: PathBuf,
    pub runner_temp: PathBuf,
    pub deadline: CheckDeadline,
}

#[derive(Debug, Serialize)]
pub struct AdmittedMacPublication {
    producer_run_id: u64,
    producer_run_attempt: u32,
    producer_artifact_id: u64,
    publisher_artifact_id: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    schema: String, repository: String, repository_id: u64, source_commit: String,
    public_run_id: u64, public_run_attempt: u32,
    producer_run_id: u64, producer_run_attempt: u32, producer_artifact_id: u64,
    publisher_artifact_id: u64, publisher_artifact_digest: String,
    manifest_sha256: String, runner_lock_sha256: String,
}

#[derive(Deserialize)]
struct Artifact { id: u64, name: String, expired: bool, size_in_bytes: u64, digest: Option<String>, workflow_run: ArtifactRun }
#[derive(Deserialize)]
struct ArtifactRun { id: u64, head_sha: String, head_branch: String }

pub fn admit_mac_publication(request: &AdmitMacPublicationRequest) -> Result<AdmittedMacPublication, OrchestratorError> {
    let invalid = || internal("mac_publication_context_invalid");
    let token = std::env::var("GH_TOKEN").map_err(|_| invalid())?;
    let source = &request.source_commit;
    let run = request.release_run_id;
    let attempt = request.release_run_attempt;
    let deadline = request.deadline;
    if token.is_empty() || run == 0 || attempt == 0 || !request.assets_dir.is_absolute() || !request.runner_temp.is_absolute()
        || source.len() != 40 || !source.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(invalid());
    }
    if github::ci::main_sha(&token, deadline)? != *source { return Err(invalid()); }
    let (config, lock, workflow) = mac_expected::load_workflow_source(source, deadline)?;
    let mut source_files = crate::package_release_publish::read_protected_source_files(
        "tailrocks/velnor-new", 1_390_620_900, "main", source, &[".velnor/runner.lock"], 1024 * 1024, deadline,
    )?;
    let runner_lock = source_files.remove(".velnor/runner.lock").ok_or_else(invalid)?;
    let mac = config.runner_image_family.as_ref().and_then(|family| family.mac_qualification.as_ref()).ok_or_else(invalid)?;
    let version = match &mac.sdk {
        velnor_actions_contract::config::MacSdkSelection::Stable { version, .. } => version.as_str(),
        velnor_actions_contract::config::MacSdkSelection::SourcePreview {} => &lock.runtime_preview.as_ref().ok_or_else(invalid)?.receipt.version,
    };
    mac_expected::verify_workflow(&config, version, &workflow)?;
    github::authenticate_publication_context_run(source, run, attempt, &token, deadline)?;
    let id = github::context_artifact_id(run, attempt, &token, deadline)?;
    let route = format!("repos/tailrocks/velnor-new/actions/artifacts/{id}");
    let artifact: Artifact = github::read::get_json(&route, &token, deadline)?;
    if id == 0 || artifact.id != id || artifact.name != format!("runner-image-publication-context-{attempt}") || artifact.expired
        || artifact.size_in_bytes == 0 || artifact.size_in_bytes > 65536 || artifact.workflow_run.id != run
        || artifact.workflow_run.head_sha != *source || artifact.workflow_run.head_branch != "main" { return Err(invalid()); }
    let bytes = github::read::get_bytes(&format!("{route}/zip"), &token, deadline)?;
    if bytes.len() as u64 != artifact.size_in_bytes || artifact.digest.as_deref() != Some(&sha(&bytes)) { return Err(invalid()); }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    zip.validate_header_consistency().map_err(|_| invalid())?;
    if zip.len() != 1 { return Err(invalid()); }
    let mut entry = zip.by_name("runner-image-publication-context.json").map_err(|_| invalid())?;
    if !entry.is_file() || entry.size() > 8192 || entry.unix_mode().is_some_and(|mode| mode & 0o170_000 == 0o120_000) { return Err(invalid()); }
    let declared = entry.size();
    let mut data = Vec::new();
    (&mut entry).take(8193).read_to_end(&mut data).map_err(|_| invalid())?;
    if data.len() as u64 != declared || data.len() > 8192 { return Err(invalid()); }
    let value = velnor_actions_contract::strict_json::parse_strict_json_bytes(&data, 8192).map_err(|_| invalid())?;
    let context: Context = serde_json::from_value(value).map_err(|_| invalid())?;
    if context.schema != "velnor.runner-image-publication-context.v1" || context.repository != "tailrocks/velnor-new"
        || context.repository_id != 1_390_620_900 || context.source_commit != *source || context.public_run_id != run
        || context.public_run_attempt != attempt || context.producer_run_id == 0 || context.producer_run_attempt == 0
        || context.producer_artifact_id == 0 || context.publisher_artifact_id == 0 || context.runner_lock_sha256 != sha(&runner_lock) { return Err(invalid()); }
    let publisher_route = format!("repos/tailrocks/velnor-new/actions/artifacts/{}", context.publisher_artifact_id);
    let publisher: Artifact = github::read::get_json(&publisher_route, &token, deadline)?;
    if publisher.digest.as_deref() != Some(&context.publisher_artifact_digest) { return Err(invalid()); }
    let verified = mac_signer::verify_publisher_artifact(&mac_signer::VerifyMacAttestationRequest {
        producer_run_id: context.producer_run_id, producer_run_attempt: context.producer_run_attempt,
        producer_artifact_id: context.producer_artifact_id, publisher_artifact_id: context.publisher_artifact_id,
        assets_dir: request.assets_dir.clone(), runner_temp: request.runner_temp.clone(), deadline,
    }, source, run, attempt, &token)?;
    if context.manifest_sha256 != sha(&verified.manifest) { return Err(invalid()); }
    github::ci::authenticate_captured(source, context.producer_run_id, context.producer_run_attempt, &token, deadline)?;
    github::authenticate_runner_producer_artifact(source, context.producer_run_id, context.producer_run_attempt, context.producer_artifact_id, &token, deadline)?;
    github::authenticate_publication_context_run(source, run, attempt, &token, deadline)?;
    if github::ci::main_sha(&token, deadline)? != *source { return Err(invalid()); }
    Ok(AdmittedMacPublication { producer_run_id: context.producer_run_id, producer_run_attempt: context.producer_run_attempt,
        producer_artifact_id: context.producer_artifact_id, publisher_artifact_id: context.publisher_artifact_id })
}

fn sha(bytes: &[u8]) -> String { format!("sha256:{}", mac_signer::hex(&Sha256::digest(bytes))) }
