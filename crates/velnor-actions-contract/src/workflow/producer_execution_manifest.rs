// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::TaskExecutionManifestV1;
use crate::{ContractError, validate_digest};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const PRODUCER_EXECUTION_MANIFEST_PATH: &str = ".github/velnor/producer-executions-v1.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerExecutionManifest {
    pub schema: u32,
    pub task_manifest_digest: String,
    pub producer_tasks: BTreeMap<String, BTreeMap<String, String>>,
}

impl ProducerExecutionManifest {
    pub fn validate(&self, tasks: &TaskExecutionManifestV1) -> Result<(), ContractError> {
        if self.schema != 1 {
            return Err(ContractError::identity(
                "producer_execution_manifest",
                "unsupported_schema",
            ));
        }
        validate_digest(&self.task_manifest_digest)?;
        let available: BTreeSet<_> = tasks
            .tasks
            .values()
            .map(|record| (record.task_id.as_str(), record.execution_digest.as_str()))
            .collect();
        for (job, records) in &self.producer_tasks {
            super::validate_job_id(job)?;
            if !job.starts_with(super::state_seal::STATE_PRODUCER_JOB_ID_PREFIX)
                || records.is_empty()
            {
                return Err(ContractError::identity(
                    "producer_execution_manifest",
                    "invalid_producer",
                ));
            }
            for (task, digest) in records {
                if !available.contains(&(task.as_str(), digest.as_str())) {
                    return Err(ContractError::identity(
                        "producer_execution_manifest",
                        "missing_execution",
                    ));
                }
            }
        }
        Ok(())
    }
}
