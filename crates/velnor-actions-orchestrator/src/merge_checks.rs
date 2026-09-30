//! `merge-v1` plan checks: matrix agreement, plan shape, evidence
//! revalidation, Execute inventory, and shard-proof validation.
//!
//! Declared from `merge.rs` (`#[path]`, no `lib.rs` edit) beside the
//! inventory checks in `required_evidence.rs`.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ExecuteTaskRef, MatrixEntry, ObligationDecision, Plan, PlanMatrix, canonical_json_bytes,
};

use super::MergeRequest;
use crate::OrchestratorError;
use crate::cover::Signals;
use crate::cover::revalidate_coverage;
use crate::cover::shard::{check_entry_shards, validate_budgets};
use crate::internal::internal_contract;

/// Check 1: `matrix.json` agrees with the plan matrix (WF-4.16).
pub(crate) fn check_agreement(
    matrix: Option<&PlanMatrix>,
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) -> Result<(), OrchestratorError> {
    let Some(matrix) = matrix else {
        signals.planning_failed = true;
        miss_reasons.insert("source_missing".to_owned());
        return Ok(());
    };
    let matrix_bytes = canonical_json_bytes(matrix).map_err(internal_contract)?;
    if velnor_actions_contract::check_matrix_agreement(&plan.matrix, &matrix_bytes).is_err() {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
    Ok(())
}

/// The no-work decision needs obligations and task IDs to agree.
pub(crate) fn check_plan_shape(
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    if plan.obligations.is_empty() != plan.task_ids.is_empty() {
        signals.planning_failed = true;
        miss_reasons.insert("cache_corrupt".to_owned());
    }
}

/// Every Execute obligation needs a matrix leg; hollow plans fail closed.
///
/// Coverage walks `matrix.include`, so an Execute obligation without a
/// leg would pass with zero task evidence. Baseline-covered and
/// cache-reused dispositions need no leg. The match stays exhaustive so
/// a future decision variant fails to compile here instead of slipping
/// through unchecked.
pub(crate) fn check_execute_inventory(
    plan: &Plan,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    let mut leg_tasks = BTreeSet::new();
    for entry in &plan.matrix.include {
        for task_ref in entry.execute_task_ids.tasks.values() {
            let ids: &[String] = match task_ref {
                ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
                ExecuteTaskRef::Shards(ids) => ids.as_slice(),
            };
            // Leg IDs match obligation IDs verbatim: sharded obligations
            // carry their full `/shard-N-of-M` IDs, never the bare base.
            for id in ids {
                leg_tasks.insert(id.as_str());
            }
        }
    }
    let mut hollow = false;
    for obligation in &plan.obligations {
        match obligation.decision {
            ObligationDecision::Execute => {
                hollow = hollow || !leg_tasks.contains(obligation.task_id.as_str());
            }
            ObligationDecision::ReusedFromTaskCache
            | ObligationDecision::CoveredByTrustedBaseline => {}
        }
    }
    if hollow {
        signals.planning_failed = true;
        miss_reasons.insert("no_entry".to_owned());
    }
}

/// Expected entries keyed by report ID, sorted.
pub(crate) fn plan_entries(plan: &Plan) -> BTreeMap<&str, &MatrixEntry> {
    plan.matrix
        .include
        .iter()
        .map(|entry| (entry.report_id.as_str(), entry))
        .collect()
}

/// Compare the planned obligation set with the sequential reference.
fn reference_matches(reference: &[String], planned: &[String]) -> bool {
    let mut left = reference.to_vec();
    let mut right = planned.to_vec();
    left.sort();
    right.sort();
    left == right
}

/// Revalidate planner coverage, limits, and reference obligations.
pub(crate) fn check_plan_evidence(
    plan: &Plan,
    request: &MergeRequest,
    signals: &mut Signals,
    miss_reasons: &mut BTreeSet<String>,
) {
    revalidate_coverage(
        plan,
        request.baseline_manifest.as_ref(),
        signals,
        miss_reasons,
    );
    if request
        .limits
        .as_ref()
        .is_some_and(|limits| validate_budgets(limits).is_err())
    {
        signals.planning_failed = true;
    }
    if request
        .reference_task_ids
        .as_ref()
        .is_some_and(|reference| !reference_matches(reference, &plan.task_ids))
    {
        signals.planning_failed = true;
    }
}

/// True when an entry's shard proofs fail validation.
pub(crate) fn shards_failed(
    entry: &MatrixEntry,
    empty: u32,
    plan: &Plan,
    request: &MergeRequest,
) -> bool {
    let bases = sharded_bases(entry);
    if bases.is_empty() {
        return false;
    }
    let mut inputs = BTreeMap::new();
    for ob in &plan.obligations {
        inputs.insert(ob.task_id.clone(), ob.input_digest.clone());
    }
    check_entry_shards(&bases, empty, &request.shard_proofs, &inputs).is_err()
}

/// Base task IDs carrying shard suffixes in one entry.
fn sharded_bases(entry: &MatrixEntry) -> BTreeSet<String> {
    let mut bases = BTreeSet::new();
    for task_ref in entry.execute_task_ids.tasks.values() {
        let ids = match task_ref {
            velnor_actions_contract::ExecuteTaskRef::Single(id) => std::slice::from_ref(id),
            velnor_actions_contract::ExecuteTaskRef::Shards(ids) => ids.as_slice(),
        };
        for id in ids {
            if let Some((base, _)) = id.split_once("/shard-") {
                bases.insert(base.to_owned());
            }
        }
    }
    bases
}

/// Obligation task digests keyed by task ID.
pub(crate) fn plan_digests(plan: &Plan) -> BTreeMap<&str, &str> {
    plan.obligations
        .iter()
        .map(|obligation| (obligation.task_id.as_str(), obligation.task_digest.as_str()))
        .collect()
}
