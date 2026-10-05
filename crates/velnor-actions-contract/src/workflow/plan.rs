//! Schema-1 affected plan and generic matrix entries.
use super::baseline::{BaselineProof, PlanBaseline};
use super::lanes::NamedCheckLaneVariant;
use super::matrix_entry::MatrixEntry;
use super::trust::Trust;
use crate::canonical::validate_digest;
use crate::config::{RUNNER_LABEL_CATALOG, RunnerSelection};
use crate::errors::ContractError;
use crate::graph::{
    TaskEdge, check_sorted, check_sorted_by, check_sorted_unique, validate_plan_edges,
};
use crate::ids::{plan_id_for_run, validate_run_key, validate_task_id};
use serde::{Deserialize, Serialize};

/// Plan-step marker selecting output limits for an output-fed matrix job.
///
/// The renderer adds this environment variable only when the plan's
/// `matrix` step output becomes a GitHub job output consumed by
/// `strategy.matrix`.
pub const PLAN_MATRIX_OUTPUT_MODE_ENV: &str = "VELNOR_PLAN_MATRIX_OUTPUT_MODE";
/// Exact value of [`PLAN_MATRIX_OUTPUT_MODE_ENV`] for dynamic matrices.
pub const DYNAMIC_MATRIX_OUTPUT_MODE: &str = "dynamic_matrix";
use std::collections::{BTreeMap, BTreeSet};
/// Schema-1 affected plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    /// Plan schema version; must be 1.
    pub schema: u32,
    /// Run key.
    pub run_key: String,
    /// Derived plan ID.
    pub plan_id: String,
    /// Base commit or null.
    pub base: Option<String>,
    /// Head commit.
    pub head: String,
    /// Triggering event.
    pub event: WorkflowEvent,
    /// Selected runner.
    pub runner: PlanRunner,
    /// Trust scope.
    pub trust: Trust,
    /// Baseline evidence.
    pub baseline: PlanBaseline,
    /// Generator identity.
    pub generator: PlanGenerator,
    /// Complete package inventory (sorted).
    pub packages: Vec<PlanPackage>,
    /// Every current obligation (sorted).
    pub obligations: Vec<PlanObligation>,
    /// Bounded matrix.
    pub matrix: PlanMatrix,
    /// Every obligation ID (sorted unique).
    pub task_ids: Vec<String>,
    /// Plan warnings.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Typed graph edges incl. resource exclusions (par §3, §7).
    #[serde(default)]
    pub edges: Vec<TaskEdge>,
}
/// Triggering workflow event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEvent {
    /// Pull request.
    PullRequest,
    /// Branch push.
    Push,
    /// Merge group.
    MergeGroup,
    /// Local pre-push run (tracked+staged+untracked+deletions).
    Local,
    /// Fork pull-request run (untrusted, read-only caches).
    Fork,
}
/// Selected runner record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRunner {
    /// Literal runner label.
    pub label: String,
    /// Selection provenance.
    pub selection: RunnerSelection,
}
/// Generator identity record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanGenerator {
    /// Exact generator version.
    pub version: String,
    /// Generator target triple.
    pub target: String,
    /// Generator SHA-256 (64 lowercase hex).
    pub sha256: String,
}
/// One inventoried package.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanPackage {
    /// Cargo package ID.
    pub package_id: String,
    /// Package name.
    pub name: String,
    /// Repo-relative manifest path.
    pub manifest: String,
    /// True when the package has an execute obligation.
    pub selected: bool,
    /// Selection reasons (sorted).
    pub reasons: Vec<String>,
    /// Package task IDs (sorted).
    pub tasks: Vec<String>,
}
/// One planned obligation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanObligation {
    /// Obligation task ID.
    pub task_id: String,
    /// Coverage decision.
    pub decision: ObligationDecision,
    /// Decision reason.
    pub reason: String,
    /// Task digest.
    pub task_digest: String,
    /// Input digest.
    pub input_digest: String,
    /// Canonical digest over the task's complete input closure.
    ///
    /// The plan path resolves the closure against the checkout and binds
    /// this digest into `input_digest`; baseline publishers copy it into
    /// their entries so coverage can compare closures explicitly instead
    /// of trusting the changed-work hint.
    pub closure_digest: String,
    /// Baseline proof (required when covered).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_proof: Option<BaselineProof>,
}
/// Obligation coverage decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObligationDecision {
    /// Must execute in the matrix.
    Execute,
    /// Reused from the task cache.
    ReusedFromTaskCache,
    /// Covered by the trusted baseline.
    CoveredByTrustedBaseline,
}
/// Bounded matrix payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanMatrix {
    /// Matrix entries (sorted by `id`).
    pub include: Vec<MatrixEntry>,
}
impl PlanRunner {
    /// Validate the recorded label against the exact-label catalog.
    /// # Errors
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
    /// Plan schema version.
    pub const SCHEMA: u32 = 1;
    /// Validate schema, plan ID, sorting, digests, and matrix entries.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema != Self::SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "1",
            });
        }
        validate_run_key(&self.run_key)?;
        if plan_id_for_run(&self.run_key)? != self.plan_id {
            return Err(ContractError::identity("plan_id", "plan_mismatch"));
        }
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
    /// Validate one obligation record.
    fn validate(&self) -> Result<(), ContractError> {
        validate_task_id(&self.task_id)?;
        validate_digest(&self.task_digest)?;
        validate_digest(&self.input_digest)?;
        validate_digest(&self.closure_digest)?;
        if self.reason.trim().is_empty() {
            return Err(ContractError::identity(
                "obligations.reason",
                "empty_reason",
            ));
        }
        let covered = self.decision == ObligationDecision::CoveredByTrustedBaseline;
        if covered && self.baseline_proof.is_none() {
            return Err(ContractError::identity(
                "obligations.baseline_proof",
                "missing_proof",
            ));
        }
        if let Some(proof) = &self.baseline_proof {
            proof.validate()?;
        }
        Ok(())
    }
}
/// Validate one leg command: nonempty single line (GITHUB_OUTPUT-safe).
/// # Errors
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

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;

/// Check `task_ids` contains every obligation ID, nothing else (wf §4).
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
