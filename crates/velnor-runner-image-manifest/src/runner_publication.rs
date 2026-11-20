// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::ImageContractError as ContractError;

use crate::candidate::{ImageCandidateArchive, ImageCandidateOci, ImageCandidateWorkflow};

pub const RUNNER_IMAGE_PUBLICATION_SCHEMA: &str = "velnor.runner-image-publication.v2";
pub const RUNNER_IMAGE_FAMILY_ARTIFACT_NAMES: [&str; 13] = [
    "velnor-runner-linux-amd64.tar",
    "velnor-dind-linux-amd64.tar",
    "velnor-resource-probe-linux-amd64.tar",
    "runner-image-provenance.json",
    "dind-image-provenance.json",
    "resource-probe-image-provenance.json",
    "runner-image-publication.json",
    "dind-image-publication.json",
    "resource-probe-image-publication.json",
    "resource_probe",
    "storage_init",
    "job_start_hook",
    "job_started_hook",
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
pub struct RunnerImageNamedContext {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImagePublicationProvenance {
    pub envelope: ImageCandidateArchive,
    pub original_archive: ImageCandidateArchive,
    pub original_index: ImageCandidateArchive,
    pub named_contexts: BTreeMap<String, RunnerImageNamedContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImagePublicationRecord {
    pub schema: String,

    pub image: String,

    pub source: RunnerImagePublicationSource,

    pub workflow: ImageCandidateWorkflow,

    pub platform: String,

    pub archive: ImageCandidateArchive,

    pub oci: ImageCandidateOci,

    pub provenance: RunnerImagePublicationProvenance,

    pub lock: RunnerImagePublicationLock,
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
    if !valid_provenance(&record.provenance, &record.image) {
        return Err(invalid("provenance"));
    }
    validate_lock_identity(record)
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
    if is_hex(&workflow.authority_commit, 40)
        && workflow.repository == "tailrocks/velnor-new"
        && workflow.repository_id == 1_390_620_900
        && workflow.event == "push"
        && workflow.run_id != 0
        && workflow.run_attempt != 0
        && workflow.workflow_ref == "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main"
        && workflow.authority_commit == record.source.commit
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
    let expected = crate::inventory::runner_publication_source_inputs()?
        .iter()
        .map(|path| (*path).to_owned())
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

pub(super) fn valid_provenance(value: &RunnerImagePublicationProvenance, image: &str) -> bool {
    let names: &[&str] = match image {
        "runner" => &[
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ],
        "dind" | "resource_probe" => &["resource_probe"],
        _ => return false,
    };
    let bounded = |item: &ImageCandidateArchive, limit: u64| {
        is_hex(&item.sha256, 64) && item.size_bytes > 0 && item.size_bytes <= limit
    };
    bounded(&value.envelope, 8 * 1024 * 1024)
        && bounded(&value.original_archive, 4 * 1024 * 1024 * 1024)
        && bounded(&value.original_index, 1024 * 1024)
        && value.named_contexts.len() == names.len()
        && names.iter().all(|name| {
            value.named_contexts.get(*name).is_some_and(|context| {
                context.path == format!("target/velnor-runner-family/{name}")
                    && is_hex(&context.sha256, 64)
                    && context.size_bytes > 0
                    && context.size_bytes <= 1024 * 1024 * 1024
            })
        })
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
        .any(|version| !crate::is_exact_semver(version))
    {
        return Err(invalid("lock_identity"));
    }
    if record.source.inputs.get(".velnor/runner.lock") != Some(&lock.sha256) {
        return Err(invalid("lock_source_digest"));
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
