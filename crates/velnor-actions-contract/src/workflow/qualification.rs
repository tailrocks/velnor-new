// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::errors::ContractError;
use crate::ids::{
    artifact_id_for_final, validate_artifact_id, validate_report_id, validate_run_key,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalReport {
    pub schema: u32,

    pub report_id: String,

    pub run_key: String,

    pub plan_id: String,

    pub expected_report_ids: Vec<String>,

    pub downloaded_artifact_ids: Vec<String>,

    pub required_job_results: Vec<RequiredJobResult>,

    pub status: FinalStatus,

    pub counts: FinalCounts,

    #[serde(default)]
    pub miss_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredJobResult {
    pub job_id: String,

    pub conclusion: JobConclusion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobConclusion {
    Success,

    Failure,

    Cancelled,

    Skipped,

    Neutral,

    Missing,
}

impl JobConclusion {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
            Self::Skipped => "skipped",
            Self::Neutral => "neutral",
            Self::Missing => "missing",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "success" => Ok(Self::Success),
            "failure" => Ok(Self::Failure),
            "cancelled" => Ok(Self::Cancelled),
            "skipped" => Ok(Self::Skipped),
            "neutral" => Ok(Self::Neutral),
            "missing" => Ok(Self::Missing),
            _ => Err(ContractError::identity(
                "conclusion",
                format!("unknown_conclusion:{value}"),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalStatus {
    NoWork,

    Passed,

    Failed,

    Cancelled,

    Blocked,

    NotRun,

    PlanningFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalCounts {
    pub selected: u32,

    pub reused: u32,

    pub executed: u32,

    pub empty_partition: u32,

    pub covered: u32,

    pub not_affected: u32,

    pub failed: u32,

    pub cancelled: u32,

    pub blocked: u32,

    pub not_run: u32,
}

pub fn final_report_id_for_run(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(format!("final-{run_key}"))
}

pub fn final_report_relpath(run_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    Ok(format!("velnor/{run_key}/final-report.json"))
}

pub fn matrix_report_relpath(run_key: &str, matrix_key: &str) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    crate::ids::validate_matrix_key(matrix_key)?;
    Ok(format!("velnor/{run_key}/{matrix_key}/matrix-report.json"))
}

pub fn task_report_relpath(
    run_key: &str,
    matrix_key: &str,
    task_report_id: &str,
) -> Result<String, ContractError> {
    validate_run_key(run_key)?;
    crate::ids::validate_matrix_key(matrix_key)?;
    crate::ids::validate_task_report_id(task_report_id)?;
    Ok(format!(
        "velnor/{run_key}/{matrix_key}/tasks/{task_report_id}.json"
    ))
}

pub fn join_runner_temp(runner_temp: &str, relpath: &str) -> Result<String, ContractError> {
    if runner_temp.trim().is_empty() {
        return Err(ContractError::identity("runner_temp", "empty_temp_dir"));
    }
    if runner_temp.contains("..") {
        return Err(ContractError::identity("runner_temp", "parent_traversal"));
    }
    if relpath.is_empty() || relpath.starts_with('/') || relpath.contains('\\') {
        return Err(ContractError::identity("relpath", "non_relative_path"));
    }
    if relpath.split('/').any(|seg| seg.is_empty() || seg == "..") {
        return Err(ContractError::identity("relpath", "parent_traversal"));
    }
    Ok(format!("{}/{}", runner_temp.trim_end_matches('/'), relpath))
}

pub fn validate_final_report_id(value: &str) -> Result<(), ContractError> {
    let Some(run_key) = value.strip_prefix("final-") else {
        return Err(ContractError::identity(
            "report_id",
            "malformed_final_report_id",
        ));
    };
    validate_run_key(run_key)
        .map_err(|_| ContractError::identity("report_id", "malformed_final_report_id"))
}
impl FinalReport {
    pub const SCHEMA: u32 = 1;

    pub fn without_plan(
        run_key: &str,
        required_job_results: Vec<RequiredJobResult>,
    ) -> Result<Self, ContractError> {
        validate_run_key(run_key)?;
        let mut jobs = required_job_results;
        jobs.sort_by(|left, right| left.job_id.cmp(&right.job_id));
        Ok(Self {
            schema: Self::SCHEMA,
            report_id: final_report_id_for_run(run_key)?,
            run_key: run_key.to_owned(),
            plan_id: crate::ids::plan_id_for_run(run_key)?,
            expected_report_ids: Vec::new(),
            downloaded_artifact_ids: Vec::new(),
            required_job_results: jobs,
            status: FinalStatus::PlanningFailed,
            counts: FinalCounts {
                selected: 0,
                reused: 0,
                executed: 0,
                empty_partition: 0,
                covered: 0,
                not_affected: 0,
                failed: 0,
                cancelled: 0,
                blocked: 0,
                not_run: 0,
            },
            miss_reasons: Vec::new(),
        })
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        check_schema(self.schema)?;
        validate_final_report_id(&self.report_id)?;
        validate_run_key(&self.run_key)?;
        crate::ids::validate_plan_id(&self.plan_id)?;
        if !is_sorted(&self.expected_report_ids) || !is_sorted(&self.downloaded_artifact_ids) {
            return Err(ContractError::identity("final_report", "must_be_sorted"));
        }
        for report_id in &self.expected_report_ids {
            validate_report_id(report_id)?;
        }
        for artifact_id in &self.downloaded_artifact_ids {
            validate_artifact_id(artifact_id)?;
        }
        for reason in &self.miss_reasons {
            crate::cachekey::validate_final_miss_reason(reason)?;
        }
        Ok(())
    }

    pub fn artifact_id(&self) -> Result<String, ContractError> {
        artifact_id_for_final(&self.run_key)
    }
}

fn check_schema(schema: u32) -> Result<(), ContractError> {
    if schema != 1 {
        return Err(ContractError::UnsupportedSchema {
            field: "schema",
            found: schema.to_string(),
            expected: "1",
        });
    }
    Ok(())
}

fn is_sorted(list: &[String]) -> bool {
    list.windows(2).all(|pair| pair[0] <= pair[1])
}
