use std::collections::{BTreeMap, BTreeSet};

use crate::ContractError;

/// Current wire schema consumed by the manual job-image publisher.
pub const IMAGE_CANDIDATE_SCHEMA: &str = "velnor.job-image-candidate.v1";
/// Maximum serialized candidate record size.
pub const IMAGE_CANDIDATE_MAX_RECORD_BYTES: usize = 1024 * 1024;
/// Existing declared build inputs retained for the v1 manual-publisher contract.
pub const IMAGE_CANDIDATE_SOURCE_INPUTS: [&str; 5] = [
    ".velnor/runner.lock",
    "images/job/ubuntu-26.04/Dockerfile",
    "images/job/ubuntu-26.04/README.md",
    "images/job/ubuntu-26.04/build-test.sh",
    "images/job/ubuntu-26.04/candidate/candidate.py",
];

/// Exact source commit, ref and declared input digests for one image candidate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateSource {
    /// Full lowercase Git commit.
    pub commit: String,
    /// GitHub ref used for the build.
    #[serde(rename = "ref")]
    pub source_ref: String,
    /// Existing v1 input-path to SHA-256 map.
    pub inputs: BTreeMap<String, String>,
}

/// Authenticated GitHub Actions workflow identity captured by the builder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateWorkflow {
    /// `owner/repository` slug.
    pub repository: String,
    /// Stable numeric GitHub repository ID.
    pub repository_id: u64,
    /// GitHub event name.
    pub event: String,
    /// Workflow path and ref.
    pub workflow_ref: String,
    /// Workflow definition commit.
    pub authority_commit: String,
    /// GitHub Actions run ID.
    pub run_id: u64,
    /// GitHub Actions run attempt.
    pub run_attempt: u64,
}

/// Buildx settings recorded for the candidate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateBuild {
    /// Exact Buildx version string.
    pub builder: String,
    /// Exact OCI platform.
    pub platform: String,
    /// Build arguments; currently required to be empty.
    pub arguments: Vec<String>,
    /// Exporter kind.
    pub exporter: String,
    /// `BuildKit` provenance mode.
    pub provenance: String,
    /// Layer compression mode.
    pub compression: String,
    /// Whether compression is forced for every layer.
    pub force_compression: bool,
}

/// Candidate archive byte identity.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateArchive {
    /// Lowercase archive SHA-256 hex digest.
    pub sha256: String,
    /// Archive byte length.
    pub size_bytes: u64,
}

/// OCI graph identity derived from the exported archive.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateOci {
    /// SHA-256 of `index.json` bytes.
    pub archive_index_digest: String,
    /// Length of `index.json`.
    pub archive_index_size_bytes: u64,
    /// Selected platform manifest digest.
    pub platform_manifest_digest: String,
    /// Selected platform manifest length.
    pub platform_manifest_size_bytes: u64,
    /// Image configuration digest.
    pub image_configuration_digest: String,
    /// Image configuration length.
    pub image_configuration_size_bytes: u64,
    /// Ordered layer descriptor digests.
    pub layer_digests: Vec<String>,
}

/// Candidate record shared with the existing manual publisher.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateRecord {
    /// Versioned record schema.
    pub schema: String,
    /// Source identity and declared input digests.
    pub source: ImageCandidateSource,
    /// Workflow identity.
    pub workflow: ImageCandidateWorkflow,
    /// Candidate platform.
    pub platform: String,
    /// Buildx settings.
    pub build: ImageCandidateBuild,
    /// OCI archive identity.
    pub archive: ImageCandidateArchive,
    /// Selected OCI graph identity.
    pub oci: ImageCandidateOci,
}

/// Parse, bound, and validate the strict v1 candidate record.
///
/// # Errors
/// Rejects invalid JSON, duplicate keys, unknown fields, or noncanonical identity facts.
pub fn parse_image_candidate_record(bytes: &[u8]) -> Result<ImageCandidateRecord, ContractError> {
    let value =
        crate::strict_json::parse_strict_json_bytes(bytes, IMAGE_CANDIDATE_MAX_RECORD_BYTES)?;
    let record: ImageCandidateRecord = serde_json::from_value(value)
        .map_err(|error| ContractError::CanonicalJson(format!("image_candidate:{error}")))?;
    validate_image_candidate_record(&record)?;
    Ok(record)
}

/// Validate every v1 field without performing filesystem or workflow reads.
///
/// # Errors
/// Rejects any field or cross-field identity that differs from the closed v1 policy.
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

/// Serialize the valid v1 record with sorted keys and the publisher's final LF.
///
/// # Errors
/// Rejects invalid records or canonical JSON serialization failures.
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
