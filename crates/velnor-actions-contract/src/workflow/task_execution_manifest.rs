//! Bounded, versioned data for generated declared-task wrappers.
//!
//! The manifest stores task inputs once. A wrapper selects one entry by its
//! full execution digest and derives the stable task ID from that record; the
//! existing plan digest remains a separate binding to the planner's obligation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES;
use crate::validate_digest;

use super::step::{TaskExecutionValidation, validate_task_execution};

/// Fixed generated path for the versioned task execution manifest.
pub const TASK_EXECUTION_MANIFEST_PATH: &str = ".github/velnor/task-execution-manifest-v1.json";
/// Current manifest schema.
pub const TASK_EXECUTION_MANIFEST_SCHEMA: u32 = 1;
/// Maximum distinct task records in one manifest.
///
/// This leaves more than fourfold headroom over the 897 distinct records in
/// the measured current generated workflow; total manifest bytes have their
/// own 8 MiB cap below.
pub const MAX_TASK_EXECUTION_RECORDS: usize = 4096;
/// Maximum bytes emitted for one wrapper record, including framing.
pub const MAX_TASK_EXECUTION_FRAME_BYTES: usize = 1024 * 1024;
/// First NUL-terminated field in the wrapper data protocol.
pub const TASK_EXECUTION_FRAME_MAGIC: &str = "VELNOR-TASK-EXECUTION-V1";

const MARKER_SUFFIX: &str = "; edit .velnor/config.toml and regenerate.";
const FRAME_END: &str = "END";

/// Versioned manifest of typed task executions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskExecutionManifestV1 {
    /// Manifest schema; must equal [`TASK_EXECUTION_MANIFEST_SCHEMA`].
    pub schema: u32,
    /// Exact renderer version named by the generated-file header.
    pub generator_version: String,
    /// Task records indexed by their stable task ID.
    pub tasks: BTreeMap<String, TaskExecutionManifestEntryV1>,
}

/// One task's full execution data and separate planner binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskExecutionManifestEntryV1 {
    /// Stable task ID and manifest map key.
    pub task_id: String,
    /// Digest over every execution field below, including `task_digest`.
    pub execution_digest: String,
    /// Existing digest for the planner's task obligation.
    pub task_digest: String,
    /// Canonical pinned tools and driver identity.
    pub toolchain_inputs: crate::cachekey::ToolchainInputs,
    /// Fixed, element-preserving argv.
    pub argv: Vec<String>,
    /// Fixed environment, serialized with sorted keys.
    pub env: BTreeMap<String, String>,
    /// Stable matrix identity.
    pub matrix_id: String,
    /// Short matrix lookup key.
    pub matrix_key: String,
    /// Version of the staged task-report helper.
    pub report_helper_version: String,
    /// Optional cap for the containing matrix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix_max_parallel: Option<u32>,
}

impl TaskExecutionManifestV1 {
    /// Build the exact generated-file header for this manifest version.
    ///
    /// # Errors
    ///
    /// Returns a contract error when the version spelling is invalid.
    pub fn marker_line(&self) -> Result<String, ContractError> {
        task_execution_manifest_marker_line(&self.generator_version)
    }

    /// Validate the schema, bounds, map keys, task contracts, and digests.
    ///
    /// # Errors
    ///
    /// Returns a contract error for unsupported schemas or malformed records.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != TASK_EXECUTION_MANIFEST_SCHEMA {
            return Err(ContractError::identity(
                "task_execution_manifest.schema",
                "unsupported_schema",
            ));
        }
        validate_marker_version(&self.generator_version)?;
        if self.tasks.len() > MAX_TASK_EXECUTION_RECORDS {
            return Err(ContractError::identity(
                "task_execution_manifest.tasks",
                "too_many_records",
            ));
        }
        for (task_id, record) in &self.tasks {
            if task_id != &record.task_id {
                return Err(ContractError::identity(
                    "task_execution_manifest.tasks",
                    "task_key_mismatch",
                ));
            }
            record.validate()?;
        }
        Ok(())
    }

    /// Encode the marker-prefixed canonical JSON document for generated output.
    ///
    /// # Errors
    ///
    /// Returns a contract error for invalid data or an oversized document.
    pub fn marked_json(&self) -> Result<String, ContractError> {
        self.validate()?;
        let header = self.marker_line()?;
        let body = canonical_json_bytes(self)?;
        let total = header
            .len()
            .saturating_add(1)
            .saturating_add(body.len())
            .saturating_add(1);
        if total > MAX_UNTRUSTED_DOCUMENT_BYTES {
            return Err(ContractError::identity(
                "task_execution_manifest",
                "document_too_large",
            ));
        }
        let body = String::from_utf8(body)
            .map_err(|_| ContractError::CanonicalJson("non_utf8_canonical_json".to_owned()))?;
        Ok(format!("{header}\n{body}\n"))
    }
}

/// Build the exact generated-file marker expected for an independently supplied
/// renderer version.
///
/// The resolver uses this before parsing the manifest body, so the file cannot
/// nominate its own trusted generator version.
///
/// # Errors
///
/// Returns a contract error when the version spelling is invalid.
pub fn task_execution_manifest_marker_line(
    generator_version: &str,
) -> Result<String, ContractError> {
    validate_marker_version(generator_version)?;
    Ok(format!(
        "{}{generator_version}{MARKER_SUFFIX}",
        crate::MARKER_PREFIX
    ))
}

impl TaskExecutionManifestEntryV1 {
    /// Compute the execution digest without replacing the plan digest.
    ///
    /// # Errors
    ///
    /// Returns a contract error if canonical JSON serialization fails.
    pub fn computed_execution_digest(&self) -> Result<String, ContractError> {
        let inputs = ExecutionDigestInputs {
            task_id: &self.task_id,
            task_digest: &self.task_digest,
            toolchain_inputs: &self.toolchain_inputs,
            argv: &self.argv,
            env: &self.env,
            matrix_id: &self.matrix_id,
            matrix_key: &self.matrix_key,
            report_helper_version: &self.report_helper_version,
            matrix_max_parallel: self.matrix_max_parallel,
        };
        Ok(digest_b3(&canonical_json_bytes(&inputs)?))
    }

    /// Set `execution_digest` to the canonical digest of this record.
    ///
    /// # Errors
    ///
    /// Returns a contract error if canonical JSON serialization fails.
    pub fn refresh_execution_digest(&mut self) -> Result<(), ContractError> {
        self.execution_digest = self.computed_execution_digest()?;
        Ok(())
    }

    /// Validate this record against the task contract and both digests.
    ///
    /// # Errors
    ///
    /// Returns a contract error when any field or binding is malformed.
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_task_execution(&TaskExecutionValidation {
            argv: &self.argv,
            env: &self.env,
            task_id: &self.task_id,
            task_digest: &self.task_digest,
            toolchain_inputs: &self.toolchain_inputs,
            matrix_id: &self.matrix_id,
            matrix_key: &self.matrix_key,
            report_helper_version: &self.report_helper_version,
            matrix_max_parallel: self.matrix_max_parallel,
            job: "manifest",
        })?;
        validate_digest(&self.execution_digest)?;
        if self.execution_digest != self.computed_execution_digest()? {
            return Err(ContractError::identity(
                "task_execution_manifest.execution_digest",
                "digest_mismatch",
            ));
        }
        Ok(())
    }

    /// Encode one validated, bounded NUL-framed record for a generated wrapper.
    ///
    /// Field order is fixed by the `VELNOR-TASK-EXECUTION-V1` protocol. Counts
    /// precede their arrays and `END` must be the final field, so the reader can
    /// reject truncation and trailing data without interpreting shell syntax.
    ///
    /// # Errors
    ///
    /// Returns a contract error for invalid records or oversized frames.
    pub fn nul_frame(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let mut fields =
            Vec::with_capacity(13 + self.argv.len() + self.env.len().saturating_mul(2));
        fields.extend([
            TASK_EXECUTION_FRAME_MAGIC.to_owned(),
            self.task_id.clone(),
            self.execution_digest.clone(),
            self.task_digest.clone(),
            self.matrix_id.clone(),
            self.matrix_key.clone(),
            self.report_helper_version.clone(),
            u8::from(self.matrix_max_parallel.is_some()).to_string(),
            self.matrix_max_parallel
                .map_or_else(String::new, |cap| cap.to_string()),
            self.argv.len().to_string(),
        ]);
        fields.extend(self.argv.iter().cloned());
        fields.push(self.env.len().to_string());
        for (key, value) in &self.env {
            fields.push(key.clone());
            fields.push(value.clone());
        }
        fields.push(FRAME_END.to_owned());

        let mut bytes = Vec::new();
        for field in fields {
            if field.contains('\0') {
                return Err(ContractError::identity(
                    "task_execution_manifest.frame",
                    "nul_in_field",
                ));
            }
            bytes.extend_from_slice(field.as_bytes());
            bytes.push(0);
        }
        if bytes.len() > MAX_TASK_EXECUTION_FRAME_BYTES {
            return Err(ContractError::identity(
                "task_execution_manifest.frame",
                "frame_too_large",
            ));
        }
        Ok(bytes)
    }
}

#[derive(Serialize)]
struct ExecutionDigestInputs<'a> {
    task_id: &'a str,
    task_digest: &'a str,
    toolchain_inputs: &'a crate::cachekey::ToolchainInputs,
    argv: &'a [String],
    env: &'a BTreeMap<String, String>,
    matrix_id: &'a str,
    matrix_key: &'a str,
    report_helper_version: &'a str,
    matrix_max_parallel: Option<u32>,
}

fn validate_marker_version(version: &str) -> Result<(), ContractError> {
    let valid = !version.is_empty()
        && version.bytes().any(|byte| byte.is_ascii_digit())
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'+' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(ContractError::identity(
            "task_execution_manifest.generator_version",
            "invalid_version",
        ))
    }
}

#[cfg(test)]
#[path = "task_execution_manifest_tests.rs"]
mod tests;
