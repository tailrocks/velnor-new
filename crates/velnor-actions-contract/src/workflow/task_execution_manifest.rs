use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::strict_json::MAX_UNTRUSTED_DOCUMENT_BYTES;
use crate::validate_digest;

use super::step::{TaskExecutionValidation, validate_task_execution};

pub const TASK_EXECUTION_MANIFEST_PATH: &str = ".github/velnor/task-execution-manifest-v1.json";

pub const TASK_EXECUTION_MANIFEST_SCHEMA: u32 = 1;

pub const MAX_TASK_EXECUTION_RECORDS: usize = 4096;

pub const MAX_TASK_EXECUTION_FRAME_BYTES: usize = 1024 * 1024;

pub const TASK_EXECUTION_FRAME_MAGIC: &str = "VELNOR-TASK-EXECUTION-V1";

const MARKER_SUFFIX: &str = "; edit .velnor/config.toml and regenerate.";
const FRAME_END: &str = "END";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskExecutionManifestV1 {
    pub schema: u32,

    pub generator_version: String,

    pub tasks: BTreeMap<String, TaskExecutionManifestEntryV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskExecutionManifestEntryV1 {
    pub task_id: String,

    pub execution_digest: String,

    pub task_digest: String,

    pub toolchain_inputs: crate::cachekey::ToolchainInputs,

    pub argv: Vec<String>,

    pub env: BTreeMap<String, String>,

    pub matrix_id: String,

    pub matrix_key: String,

    pub report_helper_version: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix_max_parallel: Option<u32>,
}

impl TaskExecutionManifestV1 {
    pub fn marker_line(&self) -> Result<String, ContractError> {
        task_execution_manifest_marker_line(&self.generator_version)
    }

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

    pub fn refresh_execution_digest(&mut self) -> Result<(), ContractError> {
        self.execution_digest = self.computed_execution_digest()?;
        Ok(())
    }

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
