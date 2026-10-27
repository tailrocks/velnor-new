// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::lanes::NamedCheckLaneVariant;
pub use super::matrix_entry::MatrixEntry;
pub use super::plan_jobs::{JobDisposition, PlanJob, plan_skips_work, selects_affected_work};
use super::qualification_dispatch::QualificationDispatch;
use super::trust::Trust;
use crate::canonical::validate_digest;
use crate::config::{RUNNER_LABEL_CATALOG, RunnerSelection};
use crate::errors::ContractError;
use crate::graph::{
    TaskEdge, check_sorted, check_sorted_by, check_sorted_unique, validate_plan_edges,
};
use crate::ids::{plan_id_for_run, validate_run_key, validate_task_id};
use serde::{Deserialize, Serialize};

pub const PLAN_MATRIX_OUTPUT_MODE_ENV: &str = "VELNOR_PLAN_MATRIX_OUTPUT_MODE";

pub const DYNAMIC_MATRIX_OUTPUT_MODE: &str = "dynamic_matrix";
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: u32,

    pub run_key: String,

    pub plan_id: String,

    pub base: Option<String>,

    pub head: String,

    pub event: WorkflowEvent,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification: Option<QualificationDispatch>,

    pub runner: PlanRunner,

    pub trust: Trust,

    pub generator: PlanGenerator,

    pub packages: Vec<PlanPackage>,

    pub obligations: Vec<PlanObligation>,

    pub matrix: PlanMatrix,

    pub task_ids: Vec<String>,

    pub jobs: Vec<PlanJob>,

    #[serde(default)]
    pub warnings: Vec<String>,

    #[serde(default)]
    pub edges: Vec<TaskEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEvent {
    PullRequest,

    Push,

    MergeGroup,

    Local,

    Fork,

    Qualification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRunner {
    pub label: String,

    pub selection: RunnerSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanGenerator {
    pub version: String,

    pub target: String,

    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanPackage {
    pub package_id: String,

    pub name: String,

    pub manifest: String,

    pub selected: bool,

    pub reasons: Vec<String>,

    pub tasks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanObligation {
    pub task_id: String,

    pub decision: ObligationDecision,

    pub reason: String,

    pub task_digest: String,

    pub input_digest: String,

    pub closure_digest: String,

    pub job_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationDecision {
    Execute,

    ReusedFromTaskCache,

    NotAffected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanMatrix {
    pub include: Vec<MatrixEntry>,
}
impl PlanRunner {
    pub fn validate(&self) -> Result<(), ContractError> {
        if !RUNNER_LABEL_CATALOG.contains(&self.label.as_str()) {
            return Err(ContractError::identity(
                "runner.label",
                format!("unsupported_label:{}", self.label),
            ));
        }
        Ok(())
    }
}
impl Plan {
    pub const SCHEMA: u32 = 2;

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != Self::SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "2",
            });
        }
        validate_run_key(&self.run_key)?;
        if plan_id_for_run(&self.run_key)? != self.plan_id {
            return Err(ContractError::identity("plan_id", "plan_mismatch"));
        }
        super::qualification_dispatch::validate_plan_qualification(self)?;
        self.runner.validate()?;
        check_sorted_unique(&self.task_ids, "task_ids")?;
        check_sorted_by(&self.packages, "packages", |pkg| pkg.package_id.as_str())?;
        check_sorted_by(&self.obligations, "obligations", |ob| ob.task_id.as_str())?;
        check_sorted_by(&self.matrix.include, "matrix.include", |entry| {
            entry.id.as_str()
        })?;
        let mut matrix_ids = BTreeSet::new();
        let mut matrix_keys = BTreeSet::new();
        for entry in &self.matrix.include {
            entry.validate(&self.run_key)?;
            if !matrix_ids.insert(entry.id.as_str()) {
                return Err(ContractError::Collision(format!("matrix id {}", entry.id)));
            }
            if !matrix_keys.insert(entry.matrix_key.as_str()) {
                let detail = format!("matrix key {}", entry.matrix_key);
                return Err(ContractError::Collision(detail));
            }
        }
        validate_named_check_lane_pairs(&self.matrix.include)?;
        check_obligation_agreement(&self.task_ids, &self.obligations)?;
        for package in &self.packages {
            check_sorted(&package.reasons, "packages.reasons")?;
            check_sorted(&package.tasks, "packages.tasks")?;
        }
        for obligation in &self.obligations {
            obligation.validate()?;
        }
        validate_plan_edges(&self.edges, &self.task_ids)?;
        super::plan_jobs::validate_plan_jobs(self)?;
        Ok(())
    }
}

fn validate_named_check_lane_pairs(entries: &[MatrixEntry]) -> Result<(), ContractError> {
    let mut singles = BTreeSet::new();
    let mut pairs = BTreeMap::<&str, u8>::new();
    for entry in entries {
        match entry.lane_variant {
            None => {
                if pairs.contains_key(entry.task_id.as_str()) {
                    return Err(ContractError::identity(
                        "matrix.lane_variant",
                        "mixed_single_and_paired_lanes",
                    ));
                }
                singles.insert(entry.task_id.as_str());
            }
            Some(variant) => {
                let task_id = entry.task_id.as_str();
                if singles.contains(task_id) {
                    return Err(ContractError::identity(
                        "matrix.lane_variant",
                        "mixed_single_and_paired_lanes",
                    ));
                }
                let bit = match variant {
                    NamedCheckLaneVariant::Hosted => 1,
                    NamedCheckLaneVariant::ScaleSet => 2,
                };
                let seen = pairs.entry(task_id).or_default();
                if *seen & bit != 0 {
                    return Err(ContractError::identity(
                        "matrix.lane_variant",
                        "duplicate_named_check_lane",
                    ));
                }
                *seen |= bit;
            }
        }
    }
    if pairs.values().any(|seen| *seen != 3) {
        return Err(ContractError::identity(
            "matrix.lane_variant",
            "incomplete_named_check_lane_pair",
        ));
    }
    Ok(())
}

impl PlanObligation {
    fn validate(&self) -> Result<(), ContractError> {
        validate_task_id(&self.task_id)?;
        validate_digest(&self.task_digest)?;
        validate_digest(&self.input_digest)?;
        validate_digest(&self.closure_digest)?;
        if self.job_id.trim().is_empty() {
            return Err(ContractError::identity(
                "obligations.job_id",
                "empty_job_id",
            ));
        }
        if self.reason.trim().is_empty() {
            return Err(ContractError::identity(
                "obligations.reason",
                "empty_reason",
            ));
        }
        Ok(())
    }
}

pub fn validate_matrix_run(value: &str) -> Result<(), ContractError> {
    if value.is_empty() {
        return Err(ContractError::identity("run", "empty_run"));
    }
    if value
        .chars()
        .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(ContractError::identity("run", "multiline_run"));
    }
    Ok(())
}

fn check_obligation_agreement(
    task_ids: &[String],
    obligations: &[PlanObligation],
) -> Result<(), ContractError> {
    let mut expected: Vec<&str> = obligations
        .iter()
        .map(|obligation| obligation.task_id.as_str())
        .collect();
    expected.sort_unstable();
    let mut actual: Vec<&str> = task_ids.iter().map(String::as_str).collect();
    actual.sort_unstable();
    if expected == actual {
        Ok(())
    } else {
        Err(ContractError::identity("task_ids", "obligation_mismatch"))
    }
}
