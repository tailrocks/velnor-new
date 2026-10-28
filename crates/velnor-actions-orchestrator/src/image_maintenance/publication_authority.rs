// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, fs, io::Read, os::unix::fs::MetadataExt, path::Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    ImageCandidateArchive, ImageCandidateOci, ImageCandidateRecord, RunnerImagePublicationLock,
    RunnerImagePublicationRecord, parse_image_candidate_record,
    parse_runner_image_publication_record,
};

use crate::{OrchestratorError, internal::internal};

use super::super::{runner_profiles, validate};
use super::{ImagePublicationProfile, PublishImageRequest};
use runner_profiles::ImageKind;

#[path = "publication_authority_oci.rs"]
mod oci;

pub(super) fn inspect_archive(
    request: &PublishImageRequest,
    candidate: &CandidateRecord,
) -> Result<
    velnor_archive_guard::owned_archive_guard::oci_candidate::OciCandidateEvidence,
    OrchestratorError,
> {
    oci::inspect_archive(request, candidate)
}

pub(super) const MAX_RECORD_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub(super) const IMAGE_REPOSITORY: &str = "tailrocks/velnor-new";
pub(super) const IMAGE_REPOSITORY_ID: u64 = 1_390_620_900;

#[derive(Debug, Clone)]
pub(super) struct CandidateRecord {
    pub profile: ImagePublicationProfile,
    pub source_commit: String,
    pub source_ref: String,
    pub source_inputs: BTreeMap<String, String>,
    pub workflow: CandidateWorkflow,
    pub archive: ImageCandidateArchive,
    pub oci: ImageCandidateOci,
    pub lock: Option<RunnerImagePublicationLock>,
    pub buildx_builder: Option<String>,
    pub record_sha256: String,
}

#[derive(Deserialize)]
pub(super) struct RunnerLockVersions {
    #[serde(rename = "runner_version")]
    runner: String,
    #[serde(rename = "docker_version")]
    docker: String,
    #[serde(rename = "buildx_version")]
    buildx: String,
    #[serde(rename = "compose_version")]
    compose: String,
    #[serde(rename = "node_version")]
    node: String,
}

#[derive(Debug, Clone)]
pub(super) struct CandidateWorkflow {
    pub event: String,
    pub workflow_ref: String,
    pub authority_commit: String,
    pub run_id: u64,
    pub run_attempt: u64,
    pub referenced_workflow_path: Option<String>,
    pub referenced_workflow_ref: Option<String>,
    pub referenced_workflow_sha: Option<String>,
}

pub(super) fn load_candidate(
    request: &PublishImageRequest,
) -> Result<CandidateRecord, OrchestratorError> {
    let bytes = validate::read_limited(&request.record_path, MAX_RECORD_BYTES)?;
    let record_sha256 = hex(&Sha256::digest(&bytes));
    let candidate = match request.profile {
        ImagePublicationProfile::Job => from_job(parse_image_candidate_record(&bytes)?)?,
        ImagePublicationProfile::Runner
        | ImagePublicationProfile::Dind
        | ImagePublicationProfile::ResourceProbe => from_runner(
            parse_runner_image_publication_record(&bytes)?,
            request.profile,
        )?,
    };
    if candidate.source_commit != request.source_commit
        || candidate.workflow.run_id != request.producer_run_id
        || candidate.workflow.run_attempt != u64::from(request.producer_run_attempt)
    {
        return Err(internal("image_publication_record_request_mismatch"));
    }
    validate_subject_paths(request, &candidate)?;
    Ok(CandidateRecord {
        record_sha256,
        ..candidate
    })
}

fn from_job(record: ImageCandidateRecord) -> Result<CandidateRecord, OrchestratorError> {
    if record.source.source_ref != "refs/heads/main"
        || record.workflow.repository != IMAGE_REPOSITORY
        || record.workflow.repository_id != IMAGE_REPOSITORY_ID
        || record.workflow.event != "workflow_dispatch"
        || record.workflow.workflow_ref
            != "tailrocks/velnor-new/.github/workflows/job-image-candidate.yml@refs/heads/main"
        || record.workflow.authority_commit != record.source.commit
    {
        return Err(internal("image_publication_job_record_authority"));
    }
    Ok(CandidateRecord {
        profile: ImagePublicationProfile::Job,
        source_commit: record.source.commit,
        source_ref: record.source.source_ref,
        source_inputs: record.source.inputs,
        workflow: CandidateWorkflow {
            event: record.workflow.event,
            workflow_ref: record.workflow.workflow_ref,
            authority_commit: record.workflow.authority_commit,
            run_id: record.workflow.run_id,
            run_attempt: record.workflow.run_attempt,
            referenced_workflow_path: None,
            referenced_workflow_ref: None,
            referenced_workflow_sha: None,
        },
        archive: record.archive,
        oci: record.oci,
        lock: None,
        buildx_builder: Some(record.build.builder),
        record_sha256: String::new(),
    })
}

fn from_runner(
    record: RunnerImagePublicationRecord,
    profile: ImagePublicationProfile,
) -> Result<CandidateRecord, OrchestratorError> {
    let expected_image = image_name(profile);
    if record.image != expected_image || record.source.source_ref != "refs/heads/main" {
        return Err(internal("image_publication_runner_record_profile"));
    }
    if record.lock.sha256
        != record
            .source
            .inputs
            .get(".velnor/runner.lock")
            .cloned()
            .unwrap_or_default()
    {
        return Err(internal("image_publication_runner_lock_binding"));
    }
    Ok(CandidateRecord {
        profile,
        source_commit: record.source.commit,
        source_ref: record.source.source_ref,
        source_inputs: record.source.inputs,
        workflow: CandidateWorkflow {
            event: record.workflow.event,
            workflow_ref: record.workflow.workflow_ref,
            authority_commit: record.workflow.authority_commit,
            run_id: record.workflow.run_id,
            run_attempt: record.workflow.run_attempt,
            referenced_workflow_path: Some(record.referenced_workflow.path),
            referenced_workflow_ref: Some(record.referenced_workflow.reference),
            referenced_workflow_sha: Some(record.referenced_workflow.sha),
        },
        archive: record.archive,
        oci: record.oci,
        lock: Some(record.lock),
        buildx_builder: None,
        record_sha256: String::new(),
    })
}

fn validate_subject_paths(
    request: &PublishImageRequest,
    candidate: &CandidateRecord,
) -> Result<(), OrchestratorError> {
    if request
        .archive_path
        .file_name()
        .and_then(|name| name.to_str())
        != Some(archive_name(candidate.profile))
        || request
            .record_path
            .file_name()
            .and_then(|name| name.to_str())
            != Some(record_name(candidate.profile))
    {
        return Err(internal("image_publication_subject_filename"));
    }
    Ok(())
}

pub(super) fn verify_archive_bytes(
    request: &PublishImageRequest,
    candidate: &CandidateRecord,
) -> Result<(), OrchestratorError> {
    let (digest, size) = hash_regular_file(
        &request.archive_path,
        MAX_ARCHIVE_BYTES,
        Some(request.deadline),
    )?;
    if digest != candidate.archive.sha256 || size != candidate.archive.size_bytes {
        return Err(internal("image_publication_archive_attested_bytes"));
    }
    Ok(())
}

pub(super) fn record_file_digest(path: &Path) -> Result<String, OrchestratorError> {
    hash_regular_file(path, MAX_RECORD_BYTES, None).map(|(digest, _)| digest)
}

pub(super) fn verify_source_inputs(
    candidate: &CandidateRecord,
    source_files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), OrchestratorError> {
    if source_files.len() != source_paths(candidate.profile).len()
        || source_files.keys().ne(candidate.source_inputs.keys())
    {
        return Err(internal("image_publication_source_inventory"));
    }
    for (path, bytes) in source_files {
        if candidate.source_inputs.get(path).map(String::as_str)
            != Some(hex(&Sha256::digest(bytes)).as_str())
        {
            return Err(internal("image_publication_source_digest"));
        }
    }
    let lock_bytes = source_files
        .get(".velnor/runner.lock")
        .ok_or_else(|| internal("image_publication_lock_missing"))?;
    let lock: RunnerLockVersions = toml::from_str(
        std::str::from_utf8(lock_bytes).map_err(|_| internal("image_publication_lock_utf8"))?,
    )
    .map_err(|_| internal("image_publication_lock_parse"))?;
    if [
        &lock.runner,
        &lock.docker,
        &lock.buildx,
        &lock.compose,
        &lock.node,
    ]
    .iter()
    .any(|version| !velnor_actions_contract::config::is_exact_semver(version))
    {
        return Err(internal("image_publication_lock_versions"));
    }
    if let Some(expected) = &candidate.lock
        && (expected.sha256 != hex(&Sha256::digest(lock_bytes))
            || expected.runner_version != lock.runner
            || expected.docker_version != lock.docker
            || expected.buildx_version != lock.buildx
            || expected.compose_version != lock.compose
            || expected.node_version != lock.node)
    {
        return Err(internal("image_publication_runner_lock_identity"));
    }
    if let Some(builder) = &candidate.buildx_builder {
        let version = validate::buildx_release_version(builder)
            .ok_or_else(|| internal("image_publication_job_buildx_identity"))?;
        if version != lock.buildx {
            return Err(internal("image_publication_job_buildx_identity"));
        }
    }
    Ok(())
}

pub(super) fn source_paths(profile: ImagePublicationProfile) -> &'static [&'static str] {
    match profile {
        ImagePublicationProfile::Job => &velnor_actions_contract::IMAGE_CANDIDATE_SOURCE_INPUTS,
        ImagePublicationProfile::Runner
        | ImagePublicationProfile::Dind
        | ImagePublicationProfile::ResourceProbe => {
            &velnor_actions_contract::RUNNER_PUBLICATION_SOURCE_INPUTS
        }
    }
}

pub(super) const fn image_name(profile: ImagePublicationProfile) -> &'static str {
    match profile {
        ImagePublicationProfile::Job => "job",
        ImagePublicationProfile::Runner => "runner",
        ImagePublicationProfile::Dind => "dind",
        ImagePublicationProfile::ResourceProbe => "resource_probe",
    }
}

pub(super) const fn repository(profile: ImagePublicationProfile) -> &'static str {
    match profile {
        ImagePublicationProfile::Job => "ghcr.io/tailrocks/velnor-job",
        ImagePublicationProfile::Runner => ImageKind::Runner.repository(),
        ImagePublicationProfile::Dind => ImageKind::Dind.repository(),
        ImagePublicationProfile::ResourceProbe => ImageKind::ResourceProbe.repository(),
    }
}

pub(super) fn archive_name(profile: ImagePublicationProfile) -> &'static str {
    match profile {
        ImagePublicationProfile::Job => "job-image.oci.tar",
        ImagePublicationProfile::Runner => "velnor-runner-linux-amd64.tar",
        ImagePublicationProfile::Dind => "velnor-dind-linux-amd64.tar",
        ImagePublicationProfile::ResourceProbe => "velnor-resource-probe-linux-amd64.tar",
    }
}

pub(super) fn record_name(profile: ImagePublicationProfile) -> &'static str {
    match profile {
        ImagePublicationProfile::Job => "job-image-candidate.json",
        ImagePublicationProfile::Runner => "runner-image-publication.json",
        ImagePublicationProfile::Dind => "dind-image-publication.json",
        ImagePublicationProfile::ResourceProbe => "resource-probe-image-publication.json",
    }
}

pub(super) fn tag(source_commit: &str) -> String {
    format!("sha-{source_commit}")
}

fn hash_regular_file(
    path: &Path,
    limit: u64,
    deadline: Option<velnor_actions_mise::CheckDeadline>,
) -> Result<(String, u64), OrchestratorError> {
    let before = fs::symlink_metadata(path).map_err(|_| internal("image_publication_file_stat"))?;
    if !before.is_file() || before.file_type().is_symlink() || before.len() > limit {
        return Err(internal("image_publication_file_type"));
    }
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| internal("image_publication_file_open"))?;
    let mut file = fs::File::from(fd);
    let opened = file
        .metadata()
        .map_err(|_| internal("image_publication_file_stat"))?;
    if opened.dev() != before.dev() || opened.ino() != before.ino() {
        return Err(internal("image_publication_file_changed"));
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    let mut size = 0_u64;
    loop {
        if deadline.is_some_and(|deadline| deadline.remaining().is_err()) {
            return Err(internal("image_publication_deadline"));
        }
        let count = file
            .read(&mut buffer)
            .map_err(|_| internal("image_publication_file_read"))?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .filter(|size| *size <= limit)
            .ok_or_else(|| internal("image_publication_file_limit"))?;
        hasher.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| internal("image_publication_file_stat"))?;
    let at_path =
        fs::symlink_metadata(path).map_err(|_| internal("image_publication_file_stat"))?;
    if after.len() != size
        || after.dev() != opened.dev()
        || after.ino() != opened.ino()
        || after.mtime() != opened.mtime()
        || after.mtime_nsec() != opened.mtime_nsec()
        || after.ctime() != opened.ctime()
        || after.ctime_nsec() != opened.ctime_nsec()
        || at_path.dev() != opened.dev()
        || at_path.ino() != opened.ino()
        || at_path.file_type().is_symlink()
    {
        return Err(internal("image_publication_file_changed"));
    }
    Ok((hex(&hasher.finalize()), size))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
