// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{ContractError, ExecuteTaskRef, MatrixEntry, Stack};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTaskKind {
    HomebrewCheck,
    JobImageCandidate,
}

impl NativeTaskKind {
    pub fn from_identity(task_id: &str, kind: &str, unit: &str) -> Result<Self, ContractError> {
        crate::ids::validate_task_id(task_id)?;
        match kind {
            "native_check"
                if task_id.strip_prefix("internal/native-check/") == Some(unit)
                    && !unit.is_empty() =>
            {
                Ok(Self::HomebrewCheck)
            }
            super::JOB_IMAGE_CANDIDATE_TASK_KIND
                if task_id == super::JOB_IMAGE_CANDIDATE_TASK_ID
                    && unit == super::JOB_IMAGE_CANDIDATE_UNIT_ID =>
            {
                Ok(Self::JobImageCandidate)
            }
            _ => Err(ContractError::identity("native_task", "unknown_identity")),
        }
    }

    pub fn from_entry(entry: &MatrixEntry) -> Result<Self, ContractError> {
        if entry.stack_id != Stack::Native.id() || entry.execute_task_ids.tasks.len() != 1 {
            return Err(ContractError::identity("native_task", "execute_inventory"));
        }
        let (kind, task) = entry
            .execute_task_ids
            .tasks
            .iter()
            .next()
            .ok_or_else(|| ContractError::identity("native_task", "execute_inventory"))?;
        if !matches!(task, ExecuteTaskRef::Single(id) if id == &entry.task_id) {
            return Err(ContractError::identity("native_task", "execute_identity"));
        }
        let unit = entry
            .adapter_metadata
            .get(if kind == "native_check" {
                "check_id"
            } else {
                "unit_id"
            })
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ContractError::identity("native_task", "unit_identity"))?;
        let parsed = Self::from_identity(&entry.task_id, kind, unit)?;
        if parsed == Self::JobImageCandidate
            && (entry.job_id != super::JOB_IMAGE_CANDIDATE_JOB_ID
                || entry.planned_platform
                    != crate::PlannedPlatform::new(
                        super::JOB_IMAGE_CANDIDATE_RUNNER,
                        "x86_64-unknown-linux-gnu",
                    )?)
        {
            return Err(ContractError::identity(
                "native_task",
                "candidate_placement",
            ));
        }
        Ok(parsed)
    }
}
