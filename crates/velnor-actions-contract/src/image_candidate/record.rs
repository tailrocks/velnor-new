use std::collections::{BTreeMap, BTreeSet};

use crate::ContractError;

pub const IMAGE_CANDIDATE_SCHEMA: &str = "velnor.job-image-candidate.v1";

pub const IMAGE_CANDIDATE_MAX_RECORD_BYTES: usize = 1024 * 1024;

pub const IMAGE_CANDIDATE_SOURCE_INPUTS: [&str; 5] = [
    ".velnor/runner.lock",
    "images/job/ubuntu-26.04/Dockerfile",
    "images/job/ubuntu-26.04/README.md",
    "images/job/ubuntu-26.04/build-test.sh",
    "images/job/ubuntu-26.04/candidate/candidate.py",
];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateSource {
    pub commit: String,

    #[serde(rename = "ref")]
    pub source_ref: String,

    pub inputs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateWorkflow {
    pub repository: String,

    pub repository_id: u64,

    pub event: String,

    pub workflow_ref: String,

    pub authority_commit: String,

    pub run_id: u64,

    pub run_attempt: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateBuild {
    pub builder: String,

    pub platform: String,

    pub arguments: Vec<String>,

    pub exporter: String,

    pub provenance: String,

    pub compression: String,

    pub force_compression: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateArchive {
    pub sha256: String,

    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateOci {
    pub archive_index_digest: String,

    pub archive_index_size_bytes: u64,

    pub platform_manifest_digest: String,

    pub platform_manifest_size_bytes: u64,

    pub image_configuration_digest: String,

    pub image_configuration_size_bytes: u64,

    pub layer_digests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateRecord {
    pub schema: String,

    pub source: ImageCandidateSource,

    pub workflow: ImageCandidateWorkflow,

    pub platform: String,

    pub build: ImageCandidateBuild,

    pub archive: ImageCandidateArchive,

    pub oci: ImageCandidateOci,
}

pub fn parse_image_candidate_record(bytes: &[u8]) -> Result<ImageCandidateRecord, ContractError> {
    let value =
        crate::strict_json::parse_strict_json_bytes(bytes, IMAGE_CANDIDATE_MAX_RECORD_BYTES)?;
    let record: ImageCandidateRecord = serde_json::from_value(value)
        .map_err(|error| ContractError::CanonicalJson(format!("image_candidate:{error}")))?;
    validate_image_candidate_record(&record)?;
    Ok(record)
}

pub fn validate_image_candidate_record(record: &ImageCandidateRecord) -> Result<(), ContractError> {
    if record.schema != IMAGE_CANDIDATE_SCHEMA || record.platform != "linux/amd64" {
        return Err(invalid("schema_or_platform"));
    }
    validate_source(&record.source)?;
    validate_workflow(&record.workflow)?;
    validate_build(&record.build)?;
    validate_archive(&record.archive)?;
    validate_oci(&record.oci)
}

pub fn serialize_image_candidate_record(
    record: &ImageCandidateRecord,
) -> Result<Vec<u8>, ContractError> {
    validate_image_candidate_record(record)?;
    let mut bytes = crate::canonical_json_bytes(record)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn validate_source(source: &ImageCandidateSource) -> Result<(), ContractError> {
    if !is_hex(&source.commit, 40)
        || !source.source_ref.starts_with("refs/")
        || source
            .source_ref
            .bytes()
            .any(|byte| byte.is_ascii_control())
    {
        return Err(invalid("source_identity"));
    }
    let expected = IMAGE_CANDIDATE_SOURCE_INPUTS
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let actual = source.inputs.keys().cloned().collect::<BTreeSet<_>>();
    if actual != expected || source.inputs.values().any(|digest| !is_hex(digest, 64)) {
        return Err(invalid("source_inputs"));
    }
    Ok(())
}

fn validate_workflow(workflow: &ImageCandidateWorkflow) -> Result<(), ContractError> {
    let mut repository_parts = workflow.repository.split('/');
    let valid_repository = repository_parts.next().is_some_and(is_repository_part)
        && repository_parts.next().is_some_and(is_repository_part)
        && repository_parts.next().is_none();
    if !valid_repository
        || workflow.repository_id == 0
        || workflow.run_id == 0
        || workflow.run_attempt == 0
        || workflow.event.is_empty()
        || !workflow
            .event
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        || !workflow
            .workflow_ref
            .starts_with(&format!("{}/.github/workflows/", workflow.repository))
        || !workflow.workflow_ref.contains("@refs/")
        || !is_hex(&workflow.authority_commit, 40)
    {
        return Err(invalid("workflow_identity"));
    }
    Ok(())
}

fn validate_build(build: &ImageCandidateBuild) -> Result<(), ContractError> {
    if !is_buildx_identity(&build.builder)
        || build.platform != "linux/amd64"
        || !build.arguments.is_empty()
        || build.exporter != "oci"
        || build.provenance != "disabled"
        || build.compression != "gzip"
        || !build.force_compression
    {
        return Err(invalid("build_identity"));
    }
    Ok(())
}

fn validate_archive(archive: &ImageCandidateArchive) -> Result<(), ContractError> {
    if !is_hex(&archive.sha256, 64) || archive.size_bytes == 0 {
        return Err(invalid("archive_identity"));
    }
    Ok(())
}

fn validate_oci(oci: &ImageCandidateOci) -> Result<(), ContractError> {
    if !is_hex(&oci.archive_index_digest, 64)
        || !is_digest(&oci.platform_manifest_digest)
        || !is_digest(&oci.image_configuration_digest)
        || oci.archive_index_size_bytes == 0
        || oci.platform_manifest_size_bytes == 0
        || oci.image_configuration_size_bytes == 0
        || oci.layer_digests.is_empty()
        || oci.layer_digests.iter().any(|digest| !is_digest(digest))
    {
        return Err(invalid("oci_identity"));
    }
    Ok(())
}

fn is_buildx_identity(value: &str) -> bool {
    let Some(version) = buildx_release_version(value) else {
        return false;
    };
    let mut parts = version.split('.');
    (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}

fn buildx_release_version(value: &str) -> Option<&str> {
    let version = value.strip_prefix("github.com/docker/buildx v")?;
    let (version, suffix) = version
        .split_once(' ')
        .map_or((version, None), |(version, suffix)| (version, Some(suffix)));
    if suffix.is_some_and(|value| {
        value.is_empty() || value.bytes().any(|byte| matches!(byte, b'\n' | b'\r' | 0))
    }) {
        return None;
    }
    Some(version)
}

fn is_repository_part(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
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
    ContractError::CanonicalJson(format!("image_candidate_{problem}"))
}
