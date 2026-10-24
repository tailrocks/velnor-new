use std::collections::{BTreeMap, BTreeSet};

use crate::ContractError;

/// Schema for the separate native implementation source-closure artifact.
pub const IMAGE_NATIVE_CLOSURE_SCHEMA: &str = "velnor.job-image-native-source-closure.v1";

/// Exact tracked source inputs that implement candidate construction and validation.
pub const NATIVE_IMAGE_CANDIDATE_INPUTS: [&str; 53] = [
    "Cargo.lock",
    "Cargo.toml",
    "crates/velnor-actions-cli/src/args.rs",
    "crates/velnor-actions-cli/src/args_image_maintenance.rs",
    "crates/velnor-actions-cli/src/dispatch.rs",
    "crates/velnor-actions-cli/src/dispatch_image_maintenance.rs",
    "crates/velnor-actions-cli/src/dispatch/fileless.rs",
    "crates/velnor-actions-cli/src/dispatch/public.rs",
    "crates/velnor-actions-cli/src/main.rs",
    "crates/velnor-actions-contract/Cargo.toml",
    "crates/velnor-actions-contract/src/lib.rs",
    "crates/velnor-actions-contract/src/strict_json.rs",
    "crates/velnor-actions-contract/src/image_candidate.rs",
    "crates/velnor-actions-contract/src/image_candidate/closure.rs",
    "crates/velnor-actions-contract/src/image_candidate/record.rs",
    "crates/velnor-actions-mise/Cargo.toml",
    "crates/velnor-actions-mise/src/lib.rs",
    "crates/velnor-actions-mise/src/command.rs",
    "crates/velnor-actions-mise/src/command_cancellable.rs",
    "crates/velnor-actions-mise/src/command_cancellable_helpers.rs",
    "crates/velnor-actions-mise/src/command_env.rs",
    "crates/velnor-actions-mise/src/command_output.rs",
    "crates/velnor-actions-mise/src/command_process_group.rs",
    "crates/velnor-actions-mise/src/image_candidate_command.rs",
    "crates/velnor-actions-orchestrator/Cargo.toml",
    "crates/velnor-actions-orchestrator/src/api.rs",
    "crates/velnor-actions-orchestrator/src/error.rs",
    "crates/velnor-actions-orchestrator/src/image_maintenance.rs",
    "crates/velnor-actions-orchestrator/src/image_maintenance/build.rs",
    "crates/velnor-actions-orchestrator/src/image_maintenance/build_candidate.rs",
    "crates/velnor-actions-orchestrator/src/image_maintenance/build_install.rs",
    "crates/velnor-actions-orchestrator/src/image_maintenance/build_support.rs",
    "crates/velnor-actions-orchestrator/src/image_maintenance/source.rs",
    "crates/velnor-actions-orchestrator/src/image_maintenance/validate.rs",
    "crates/velnor-actions-orchestrator/src/lib.rs",
    "crates/velnor-actions-orchestrator/src/internal.rs",
    "crates/velnor-archive-guard/Cargo.toml",
    "crates/velnor-archive-guard/src/lib.rs",
    "crates/velnor-archive-guard/src/owned_archive_guard/mod.rs",
    "crates/velnor-archive-guard/src/owned_archive_guard/oci_candidate.rs",
    "crates/velnor-archive-guard/src/owned_archive_guard/oci_candidate/archive.rs",
    "crates/velnor-archive-guard/src/owned_archive_guard/oci_candidate/graph.rs",
    "crates/velnor-archive-guard/src/owned_archive_guard/oci_candidate/graph_support.rs",
    "crates/velnor-archive-guard/src/owned_archive_guard/oci_candidate/json.rs",
    "crates/velnor-archive-guard/src/owned_archive_guard/oci_candidate/layer.rs",
    "crates/velnor-actions-workflow-renderer/src/schema2_job_image_candidate.rs",
    "crates/velnor-actions-workflow-renderer/src/schema2_job_image_candidate_tests.rs",
    "crates/velnor-actions-workflow-renderer/src/schema2_job_image_native.rs",
    ".velnor/runner.lock",
    "images/job/ubuntu-26.04/Dockerfile",
    "images/job/ubuntu-26.04/README.md",
    "images/job/ubuntu-26.04/build-test.sh",
    "images/job/ubuntu-26.04/candidate/candidate.py",
];

/// Source-bound proof for the native candidate producer implementation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageCandidateSourceClosure {
    /// Closure schema.
    pub schema: String,
    /// Full lowercase source commit containing the implementation.
    pub source_commit: String,
    /// Exact implementation path to SHA-256 hex map.
    pub inputs: BTreeMap<String, String>,
}

impl ImageCandidateSourceClosure {
    /// Construct a closure after the caller hashes every required path.
    ///
    /// # Errors
    /// Rejects a missing, extra, unsafe, or malformed input digest.
    pub fn new(
        source_commit: String,
        inputs: BTreeMap<String, String>,
    ) -> Result<Self, ContractError> {
        let closure = Self {
            schema: IMAGE_NATIVE_CLOSURE_SCHEMA.to_owned(),
            source_commit,
            inputs,
        };
        validate_image_candidate_source_closure(&closure)?;
        Ok(closure)
    }
}

/// Strictly parse a bounded native implementation source-closure artifact.
///
/// # Errors
/// Rejects oversized, duplicate-key, malformed, unknown, or invalid closure data.
pub fn parse_image_candidate_source_closure(
    bytes: &[u8],
) -> Result<ImageCandidateSourceClosure, ContractError> {
    let value = crate::strict_json::parse_strict_json_bytes(bytes, 1024 * 1024)?;
    let closure: ImageCandidateSourceClosure = serde_json::from_value(value).map_err(|error| {
        ContractError::CanonicalJson(format!("image_candidate_closure:{error}"))
    })?;
    validate_image_candidate_source_closure(&closure)?;
    Ok(closure)
}

/// Validate exact closure identity and complete tracked-input coverage.
///
/// # Errors
/// Rejects any source identity or input path set that differs from the contract.
pub fn validate_image_candidate_source_closure(
    closure: &ImageCandidateSourceClosure,
) -> Result<(), ContractError> {
    let expected = NATIVE_IMAGE_CANDIDATE_INPUTS
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let actual = closure.inputs.keys().cloned().collect::<BTreeSet<_>>();
    if closure.schema != IMAGE_NATIVE_CLOSURE_SCHEMA
        || !is_hex(&closure.source_commit, 40)
        || actual != expected
        || closure.inputs.values().any(|digest| !is_hex(digest, 64))
    {
        return Err(ContractError::CanonicalJson(
            "image_candidate_closure_identity".to_owned(),
        ));
    }
    Ok(())
}

/// Serialize a valid closure as sorted-key canonical JSON with final LF.
///
/// # Errors
/// Rejects invalid data or serialization errors.
pub fn serialize_image_candidate_source_closure(
    closure: &ImageCandidateSourceClosure,
) -> Result<Vec<u8>, ContractError> {
    validate_image_candidate_source_closure(closure)?;
    let mut bytes = crate::canonical_json_bytes(closure)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
