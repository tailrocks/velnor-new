//! Validation implementations for the versioned report schemas.

use crate::canonical::validate_digest;
use crate::errors::ContractError;
use crate::ids::{
    matrix_key_for_id, task_report_id_for_task, validate_matrix_key, validate_report_id,
    validate_run_key, validate_task_id, validate_task_report_id,
};
use crate::workflow::matrix_entry::MatrixEntry;
use crate::workflow::report::{CacheResult, MatrixReport, TaskReport, TaskStatus};

impl TaskReport {
    /// Report schema version.
    pub const SCHEMA: u32 = 3;

    /// Validate schema, derived ID, digests, binding, and status coherence.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, Self::SCHEMA)?;
        validate_run_key(&self.run_key)?;
        validate_matrix_key(&self.matrix_key)?;
        check_matrix_id(&self.matrix_id, &self.matrix_key)?;
        validate_task_id(&self.task_id)?;
        validate_digest(&self.task_digest)?;
        self.platform_binding.validate()?;
        let expect = task_report_id_for_task(&self.run_key, &self.matrix_key, &self.task_digest)?;
        if expect != self.task_report_id {
            return Err(ContractError::identity("task_report_id", "report_mismatch"));
        }
        validate_task_report_id(&self.task_report_id)?;
        let not_selected = self.status == TaskStatus::NotSelected;
        if not_selected != self.not_selected_reason.is_some() {
            return Err(ContractError::identity(
                "not_selected_reason",
                "reason_mismatch",
            ));
        }
        let miss = self.cache.result == CacheResult::Miss;
        if miss != self.cache.miss_reason.is_some() {
            return Err(ContractError::identity(
                "cache.miss_reason",
                "reason_mismatch",
            ));
        }
        if let Some(reason) = &self.cache.miss_reason {
            crate::cachekey::validate_miss_reason(reason)?;
        }
        if self.cache.key.len() > 512 {
            return Err(ContractError::identity("cache.key", "key_too_long"));
        }
        for output in &self.outputs {
            crate::canonical::normalize_posix_path(output)?;
        }
        for (field, value) in [
            ("queue", self.queue.as_deref()),
            ("partition", self.partition.as_deref()),
            ("reason", self.reason.as_deref()),
        ] {
            if let Some(text) = value {
                crate::cachekey::validate_semantic_text(field, text)?;
            }
        }
        Ok(())
    }

    /// Validate this report against the immutable matrix placement in the plan.
    /// # Errors
    pub fn validate_for_plan_entry(&self, entry: &MatrixEntry) -> Result<(), ContractError> {
        self.validate()?;
        if self.matrix_id != entry.id || self.matrix_key != entry.matrix_key {
            return Err(ContractError::identity(
                "task_report.matrix",
                "plan_mismatch",
            ));
        }
        self.platform_binding
            .validate_for_plan(&entry.planned_platform)
    }

    /// Check that outputs list only declared task outputs.
    /// # Errors
    pub fn validate_outputs_declared(&self, declared: &[String]) -> Result<(), ContractError> {
        for output in &self.outputs {
            if !declared.contains(output) {
                return Err(ContractError::identity("outputs", "undeclared_output"));
            }
        }
        Ok(())
    }
}

impl MatrixReport {
    /// Report schema version.
    pub const SCHEMA: u32 = 1;

    /// Validate schema, derived ID, coverage, sorting, and counts.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema, Self::SCHEMA)?;
        validate_run_key(&self.run_key)?;
        validate_matrix_key(&self.matrix_key)?;
        check_matrix_id(&self.matrix_id, &self.matrix_key)?;
        validate_report_id(&self.report_id)?;
        let expect = crate::ids::report_id_for_matrix(&self.run_key, &self.matrix_key)?;
        if expect != self.report_id {
            return Err(ContractError::identity("report_id", "report_mismatch"));
        }
        if !is_sorted(&self.expected_task_ids) || !is_sorted(&self.task_report_ids) {
            return Err(ContractError::identity("matrix_report", "must_be_sorted"));
        }
        if self
            .tasks
            .windows(2)
            .any(|pair| pair[0].task_id > pair[1].task_id)
        {
            return Err(ContractError::identity(
                "matrix_report.tasks",
                "must_be_sorted",
            ));
        }
        if self.tasks.len() != self.expected_task_ids.len() {
            return Err(ContractError::identity(
                "matrix_report.tasks",
                "coverage_mismatch",
            ));
        }
        let mut report_ids: Vec<&str> = self
            .tasks
            .iter()
            .map(|task| task.task_report_id.as_str())
            .collect();
        report_ids.sort_unstable();
        let mut expected_ids: Vec<&str> = self.task_report_ids.iter().map(String::as_str).collect();
        expected_ids.sort_unstable();
        if report_ids != expected_ids {
            return Err(ContractError::identity(
                "matrix_report.tasks",
                "id_mismatch",
            ));
        }
        for task in &self.tasks {
            validate_task_report_id(&task.task_report_id)?;
            validate_task_id(&task.task_id)?;
        }
        let parts = self.reused
            + self.executed
            + self.empty_partition
            + self.not_selected
            + self.failed
            + self.cancelled;
        if parts != self.selected || self.selected as usize != self.tasks.len() {
            return Err(ContractError::identity(
                "matrix_report.counts",
                "count_mismatch",
            ));
        }
        Ok(())
    }
}

/// Check the exact schema version expected by one report type.
fn check_schema(schema: u32, expected: u32) -> Result<(), ContractError> {
    if schema != expected {
        return Err(ContractError::UnsupportedSchema {
            field: "schema",
            found: schema.to_string(),
            expected: if expected == TaskReport::SCHEMA {
                "3"
            } else {
                "1"
            },
        });
    }
    Ok(())
}

/// Check a string list is sorted.
fn is_sorted(list: &[String]) -> bool {
    list.windows(2).all(|pair| pair[0] <= pair[1])
}

/// Check key derivation from the matrix ID.
fn check_matrix_id(matrix_id: &str, matrix_key: &str) -> Result<(), ContractError> {
    if matrix_key_for_id(matrix_id)? != matrix_key {
        return Err(ContractError::identity("matrix_id", "id_mismatch"));
    }
    Ok(())
}
