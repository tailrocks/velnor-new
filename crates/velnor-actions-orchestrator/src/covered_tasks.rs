//! Covered-task export: typed dispositions to generated skip gates.
//!
//! The plan artifact already exports every covered task ID through its
//! obligations; this module encodes that set for the plan job's
//! `covered_tasks` output and builds each obligation step's generated
//! `if:` skip gate over it. Encoding wraps comma-joined sorted IDs in
//! commas (`,a,b,`), so `contains` matches whole IDs only: task IDs
//! never contain commas, and the empty set encodes as the empty
//! string, which matches nothing and executes everything.

use velnor_actions_contract::{ObligationDecision, Plan, validate_task_id};
// Re-exported: the CLI emits this exact output name (single-sourced).
pub use velnor_actions_workflow_renderer::COVERED_TASKS_OUTPUT;

use crate::OrchestratorError;
use crate::internal::internal_contract;

/// Sorted unique covered task IDs of one plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CoveredTasks {
    /// Covered IDs in sorted order.
    ids: Vec<String>,
}

/// True when the decision claims trusted-baseline coverage.
///
/// The single covered-predicate: staging, fetching, encoding, and
/// the skip gate all route through here. Exhaustive over
/// [`ObligationDecision`]: a future variant fails to compile here
/// instead of silently skipping coverage somewhere.
#[must_use]
pub(crate) fn decision_is_covered(decision: ObligationDecision) -> bool {
    match decision {
        ObligationDecision::CoveredByTrustedBaseline => true,
        ObligationDecision::Execute | ObligationDecision::ReusedFromTaskCache => false,
    }
}

/// True when any obligation claims trusted-baseline coverage.
#[must_use]
pub(crate) fn plan_has_covered(plan: &Plan) -> bool {
    plan.obligations
        .iter()
        .any(|obligation| decision_is_covered(obligation.decision))
}

impl CoveredTasks {
    /// Collect `CoveredByTrustedBaseline` obligations, sorted.
    #[must_use]
    pub(crate) fn for_plan(plan: &Plan) -> Self {
        let mut ids: Vec<String> = plan
            .obligations
            .iter()
            .filter(|obligation| decision_is_covered(obligation.decision))
            .map(|obligation| obligation.task_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        Self { ids }
    }

    /// Encode for `$GITHUB_OUTPUT`: wrapped joins, empty when none.
    #[must_use]
    pub(crate) fn encode(&self) -> String {
        if self.ids.is_empty() {
            String::new()
        } else {
            format!(",{},", self.ids.join(","))
        }
    }
}

/// True when the plan proves `task_id` by trusted baseline.
#[must_use]
pub(crate) fn covered_by_baseline(plan: &Plan, task_id: &str) -> bool {
    plan.obligations
        .iter()
        .any(|obligation| obligation.task_id == task_id && decision_is_covered(obligation.decision))
}

/// Generated `if:` gate skipping `task_id` when the plan covered it.
///
/// Evaluates over the plan job's `covered_tasks` output, which the
/// crate job reads through `needs.plan`; an absent or empty output
/// matches nothing, so unknown coverage always executes.
/// # Errors
///
/// Returns a contract error when the task ID is malformed.
pub(crate) fn skip_condition(task_id: &str) -> Result<String, OrchestratorError> {
    velnor_actions_contract::task_execution_condition(task_id).map_err(internal_contract)
}

#[cfg(test)]
#[path = "covered_tasks_tests.rs"]
mod covered_tasks_tests;
