//! Crate-job representation: ordered, individually reported obligations.
//!
//! Obligations stay per task; jobs group by crate. Gates reference
//! strictly earlier obligations, so in-crate ordering (Clippy before
//! tests) holds by construction, not by renderer convention.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::cachekey::{ToolchainInputs, toolchain_id};
use crate::canonical::{canonical_json_bytes, digest_b3};
use crate::errors::ContractError;
use crate::ids::{validate_matrix_key, validate_task_id};
use crate::validate_digest;
use crate::workflow::jobs::{
    TOFU_DISPLAY_PREFIX, TOFU_JOB_ID_PREFIX, is_safe_display_name, validate_job_id,
};

/// One logical obligation inside a crate job (per task, individually reported).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateObligation {
    /// Stable task ID (never rendered into display names).
    pub task_id: String,
    /// Stack-neutral kind word (e.g. `clippy`).
    pub kind: String,
    /// Human step name (no IDs, keys, paths, or matrix JSON).
    pub step_name: String,
    /// Task IDs that must pass first (strictly earlier obligations).
    pub gated_by: Vec<String>,
    /// Matrix key locating this obligation's reports.
    pub matrix_key: String,
    /// Task digest binding argv plus toolchain.
    pub task_digest: String,
    /// Exact toolchain identity inputs bound into the task digest.
    pub toolchain_inputs: ToolchainInputs,
    /// Fixed obligation argv.
    pub run: Vec<String>,
}

/// One crate job: ordered, individually reported obligations (P05-1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateJob {
    /// Stable collision-safe job ID.
    pub job_id: String,
    /// Display name (`Rust / <label>`, or `OpenToFu — <root>` under `tofu-`).
    pub display_name: String,
    /// Cargo package name (empty only with folder-fallback display).
    pub package_name: String,
    /// Cargo package ID.
    pub package_id: String,
    /// Package manifest path.
    pub manifest: String,
    /// Configuration name.
    pub configuration: String,
    /// Ordered obligations; gates reference strictly earlier entries.
    pub obligations: Vec<CrateObligation>,
}

impl CrateJob {
    /// Validate ordering, uniqueness, gates, and label hygiene.
    ///
    /// The display prefix partitions with the ID namespace: `tofu-`
    /// jobs render `OpenToFu — <root>`, every other crate job keeps
    /// the byte-identical `Rust / <label>` contract.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        validate_job_id(&self.job_id)?;
        let prefix = if self.job_id.starts_with(TOFU_JOB_ID_PREFIX) {
            TOFU_DISPLAY_PREFIX
        } else {
            "Rust / "
        };
        if self.display_name.trim().is_empty()
            || !self.display_name.starts_with(prefix)
            || !is_safe_display_name(&self.display_name)
        {
            return Err(ContractError::identity(
                "crate_job.display_name",
                format!("bad_display:{}", self.job_id),
            ));
        }
        if self.obligations.is_empty() {
            return Err(ContractError::identity(
                "crate_job.obligations",
                format!("empty_obligations:{}", self.job_id),
            ));
        }
        let mut seen = BTreeSet::new();
        for obligation in &self.obligations {
            obligation.validate(&self.job_id)?;
            if seen.contains(&obligation.task_id) {
                return Err(ContractError::identity(
                    "crate_job.obligations",
                    format!("duplicate_task:{}", obligation.task_id),
                ));
            }
            for gate in &obligation.gated_by {
                if !seen.contains(gate) {
                    return Err(ContractError::identity(
                        "crate_job.gates",
                        format!("unordered_gate:{}:{}", obligation.task_id, gate),
                    ));
                }
            }
            seen.insert(obligation.task_id.clone());
        }
        Ok(())
    }
}

impl CrateObligation {
    /// Validate identities, report bindings, argv, and step-name hygiene.
    fn validate(&self, job_id: &str) -> Result<(), ContractError> {
        validate_task_id(&self.task_id)?;
        validate_matrix_key(&self.matrix_key)?;
        validate_digest(&self.task_digest)?;
        let toolchain_id = toolchain_id(&self.toolchain_inputs)?;
        if task_digest_for_execution(&self.task_id, &self.run, &toolchain_id)? != self.task_digest {
            return Err(ContractError::identity(
                "crate_obligation.task_digest",
                format!("task_digest_mismatch:{job_id}"),
            ));
        }
        if self.kind.trim().is_empty() {
            return Err(ContractError::identity(
                "crate_obligation.kind",
                format!("empty_kind:{job_id}"),
            ));
        }
        if self.step_name.trim().is_empty()
            || self.step_name.contains('/')
            || self.step_name.contains("${{")
        {
            return Err(ContractError::identity(
                "crate_obligation.step_name",
                format!("bad_step_name:{job_id}"),
            ));
        }
        if self.step_name.contains(&self.task_id) {
            return Err(ContractError::identity(
                "crate_obligation.step_name",
                format!("task_id_in_label:{job_id}"),
            ));
        }
        if self.run.is_empty() || self.run.iter().any(|arg| arg.trim().is_empty()) {
            return Err(ContractError::identity(
                "crate_obligation.run",
                format!("bad_argv:{job_id}"),
            ));
        }
        Ok(())
    }
}

/// Compute the task digest from its stable identity, argv, and toolchain ID.
///
/// Shared by planned obligations and generated workflow steps so a typed
/// execution cannot swap either its pinned command or tool inputs after the
/// plan has bound the task.
///
/// # Errors
///
/// Returns a contract error when the task or toolchain identity is invalid,
/// the argument vector is malformed, or canonical digest serialization fails.
pub fn task_digest_for_execution(
    task_id: &str,
    argv: &[String],
    toolchain_id: &str,
) -> Result<String, ContractError> {
    validate_task_id(task_id)?;
    validate_digest(toolchain_id)?;
    if argv.is_empty()
        || argv
            .iter()
            .any(|arg| arg.is_empty() || arg.chars().any(|ch| matches!(ch, '\0' | '\n' | '\r')))
    {
        return Err(ContractError::identity(
            "crate_obligation.argv",
            "empty_argv",
        ));
    }
    Ok(digest_b3(&canonical_json_bytes(&TaskDigestInputs {
        task_id,
        argv,
        toolchain_id,
    })?))
}

#[derive(Serialize)]
struct TaskDigestInputs<'a> {
    task_id: &'a str,
    argv: &'a [String],
    toolchain_id: &'a str,
}

#[cfg(test)]
#[path = "crate_job_tests.rs"]
mod tests;
