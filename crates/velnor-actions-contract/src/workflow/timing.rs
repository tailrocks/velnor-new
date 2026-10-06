//! Task wall measurements and unavailable timing categories.
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Origin of a collected task wall measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskTimingSource {
    /// Elapsed wall clock from the obligation wrapper's task start timestamp.
    TaskWrapperWall,
}

/// Per-category milliseconds; unavailable measurements serialize as null.
///
/// Task wall includes child work. Categories must never be added to infer
/// critical-path or runner wall time. Only task wall is currently collected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTiming {
    /// Queue wait before dispatch.
    pub queue_ms: Option<u64>,
    /// Runner provisioning.
    pub runner_ms: Option<u64>,
    /// Task-body wall time, including any child work.
    pub task_ms: Option<u64>,
    /// Origin paired with the task-body wall measurement.
    pub task_source: Option<TaskTimingSource>,
    /// Cache restore/save handling.
    pub cache_ms: Option<u64>,
    /// Preparation before the payload.
    pub prep_ms: Option<u64>,
    /// Artifact downloads.
    pub download_ms: Option<u64>,
    /// Compiler wall time.
    pub compiler_ms: Option<u64>,
    /// MBX object handling.
    pub mbx_ms: Option<u64>,
    /// Test execution proper.
    pub test_ms: Option<u64>,
    /// Lock waits.
    pub lock_wait_ms: Option<u64>,
}

impl TaskTiming {
    /// Validate every present measurement has a supported concrete origin.
    /// # Errors
    /// Rejects missing task provenance and categories without a collector.
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.task_ms.is_some() != self.task_source.is_some() {
            return Err(ContractError::identity(
                "timing.task_source",
                "source_mismatch",
            ));
        }
        if self
            .slots()
            .iter()
            .enumerate()
            .any(|(index, slot)| index != 2 && slot.is_some())
        {
            return Err(ContractError::identity("timing", "unsupported_measurement"));
        }
        Ok(())
    }

    /// Category values in canonical order; unknown values remain absent.
    #[must_use]
    pub fn slots(&self) -> [Option<u64>; 10] {
        [
            self.queue_ms,
            self.runner_ms,
            self.task_ms,
            self.cache_ms,
            self.prep_ms,
            self.download_ms,
            self.compiler_ms,
            self.mbx_ms,
            self.test_ms,
            self.lock_wait_ms,
        ]
    }
}
