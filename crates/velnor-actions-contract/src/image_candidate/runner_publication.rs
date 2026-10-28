// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::ContractError;

use super::image_candidate_record::{
    ImageCandidateArchive, ImageCandidateOci, ImageCandidateWorkflow,
};

pub const RUNNER_IMAGE_PUBLICATION_SCHEMA: &str = "velnor.runner-image-publication.v1";

pub const RUNNER_PUBLICATION_SOURCE_INPUTS: [&str; 21] = [
    ".velnor/runner.lock",
    "images/dind/Dockerfile",
    "images/dind/LICENSE-moby",
    "images/dind/cgroup-v2-delegation.sh",
    "images/dind/entrypoint.sh",
    "images/runner/ubuntu-26.04/Dockerfile",
    "images/runner/ubuntu-26.04/clear-dockerenv.sh",
    "images/runner/ubuntu-26.04/entrypoint.sh",
    "images/runner/ubuntu-26.04/tar-absolute.sh",
    "images/runner/ubuntu-26.04/tar-dir-meta.pl",
    "images/runner/ubuntu-26.04/tar-extract-plan.sh",
    "images/runner/ubuntu-26.04/tar-extract.sh",
    "images/runner/ubuntu-26.04/tar-member-rewrite.pl",
    "images/runner/ubuntu-26.04/tar-member-stream.pl",
    "images/runner/ubuntu-26.04/tar-member.pl",
    "images/runner/ubuntu-26.04/tar-pax.pl",
    "images/runner/ubuntu-26.04/tar-shim.sh",
    "images/runner/ubuntu-26.04/wait-docker-api.sh",
    "images/runner/ubuntu-26.04/wait-docker-sock.sh",
    "images/resource-probe/Dockerfile",
    "images/resource-probe/build-image.sh",
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImagePublicationSource {
    pub commit: String,

    #[serde(rename = "ref")]
    pub source_ref: String,

    pub inputs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImagePublicationLock {
    pub sha256: String,

    pub runner_version: String,

    pub docker_version: String,

    pub buildx_version: String,

    pub compose_version: String,

    pub node_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImagePublicationQualification {
    pub job_id: String,

    pub run_id: u64,

    pub run_attempt: u32,

    pub conclusion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImagePublicationReferencedWorkflow {
    pub path: String,

    #[serde(rename = "ref")]
    pub reference: String,

    pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImagePublicationRecord {
    pub schema: String,

    pub image: String,

    pub source: RunnerImagePublicationSource,

    pub workflow: ImageCandidateWorkflow,

    pub referenced_workflow: RunnerImagePublicationReferencedWorkflow,

    pub platform: String,

    pub archive: ImageCandidateArchive,

    pub oci: ImageCandidateOci,

    pub lock: RunnerImagePublicationLock,

    pub qualification: RunnerImagePublicationQualification,
}

pub fn parse_runner_image_publication_record(
    bytes: &[u8],
) -> Result<RunnerImagePublicationRecord, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(bytes, 1024 * 1024)?;
    let record: RunnerImagePublicationRecord = serde_json::from_value(value).map_err(|error| {
        ContractError::CanonicalJson(format!("runner_image_publication:{error}"))
    })?;
    validate_runner_image_publication_record(&record)?;
    Ok(record)
}

pub fn validate_runner_image_publication_record(
    record: &RunnerImagePublicationRecord,
) -> Result<(), ContractError> {
    validate_profile(record)?;
    validate_workflow_identity(record)?;
    validate_source_identity(record)?;
    validate_oci_identity(record)?;
    validate_lock_identity(record)?;
    validate_qualification_identity(record)
}

fn validate_profile(record: &RunnerImagePublicationRecord) -> Result<(), ContractError> {
    if record.schema == RUNNER_IMAGE_PUBLICATION_SCHEMA
        && ["runner", "dind", "resource_probe"].contains(&record.image.as_str())
        && record.platform == "linux/amd64"
    {
        Ok(())
    } else {
        Err(invalid("schema_or_profile"))
    }
}

fn validate_workflow_identity(record: &RunnerImagePublicationRecord) -> Result<(), ContractError> {
    let workflow = &record.workflow;
    let referenced = &record.referenced_workflow;
    if is_hex(&workflow.authority_commit, 40)
        && workflow.repository == "tailrocks/velnor-new"
        && workflow.repository_id == 1_390_620_900
        && workflow.event == "workflow_dispatch"
        && workflow.run_id != 0
        && workflow.run_attempt != 0
        && workflow.workflow_ref
            == "tailrocks/velnor-new/.github/workflows/product-release.yml@refs/heads/main"
        && is_hex(&referenced.sha, 40)
        && referenced.reference == "refs/heads/main"
        && referenced.sha == workflow.authority_commit
        && referenced.path
            == format!(
                "tailrocks/velnor-new/.github/workflows/product-release-images.yml@{}",
                referenced.sha
            )
    {
        Ok(())
    } else {
        Err(invalid("workflow_identity"))
    }
}

fn validate_source_identity(record: &RunnerImagePublicationRecord) -> Result<(), ContractError> {
    if !is_hex(&record.source.commit, 40) || record.source.source_ref != "refs/heads/main" {
        return Err(invalid("source_identity"));
    }
    let expected = RUNNER_PUBLICATION_SOURCE_INPUTS
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let actual = record
        .source
        .inputs
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if actual == expected
        && record
            .source
            .inputs
            .values()
            .all(|digest| is_hex(digest, 64))
    {
        Ok(())
    } else {
        Err(invalid("source_inputs"))
    }
}

fn validate_oci_identity(record: &RunnerImagePublicationRecord) -> Result<(), ContractError> {
    if is_hex(&record.archive.sha256, 64)
        && record.archive.size_bytes != 0
        && is_hex(&record.oci.archive_index_digest, 64)
        && is_digest(&record.oci.platform_manifest_digest)
        && is_digest(&record.oci.image_configuration_digest)
        && record.oci.archive_index_size_bytes != 0
        && record.oci.platform_manifest_size_bytes != 0
        && record.oci.image_configuration_size_bytes != 0
        && !record.oci.layer_digests.is_empty()
        && record
            .oci
            .layer_digests
            .iter()
            .all(|digest| is_digest(digest))
    {
        Ok(())
    } else {
        Err(invalid("archive_or_oci_identity"))
    }
}

fn validate_lock_identity(record: &RunnerImagePublicationRecord) -> Result<(), ContractError> {
    let lock = &record.lock;
    if !is_hex(&lock.sha256, 64)
        || [
            &lock.runner_version,
            &lock.docker_version,
            &lock.buildx_version,
            &lock.compose_version,
            &lock.node_version,
        ]
        .iter()
        .any(|version| !crate::config::is_exact_semver(version))
    {
        return Err(invalid("lock_identity"));
    }
    if record.source.inputs.get(".velnor/runner.lock") != Some(&lock.sha256) {
        return Err(invalid("lock_source_digest"));
    }
    Ok(())
}

fn validate_qualification_identity(
    record: &RunnerImagePublicationRecord,
) -> Result<(), ContractError> {
    let qualification = &record.qualification;
    if qualification.job_id != "qualify-images"
        || qualification.run_id != record.workflow.run_id
        || u64::from(qualification.run_attempt) != record.workflow.run_attempt
        || qualification.conclusion != "success"
    {
        return Err(invalid("qualification_identity"));
    }
    Ok(())
}

fn is_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| is_hex(digest, 64))
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid(problem: &str) -> ContractError {
    ContractError::CanonicalJson(format!("runner_image_publication_{problem}"))
}
