//! Validate native skip authority using the generator's existing obligation order.

use std::collections::BTreeSet;

use velnor_actions_contract::{MatrixEntry, Plan};

use crate::OrchestratorError;
use crate::internal::internal;

/// Require precisely the selected later obligations; baseline coverage is vacuous.
pub(crate) fn check(
    plan: &Plan,
    source: &MatrixEntry,
    ids: &[String],
) -> Result<(), OrchestratorError> {
    let expected = expected(plan, source)?;
    let source_order = order(&source.stack_id, &source.task_id)?;
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();
    for id in ids {
        if id == &source.task_id || !seen.insert(id) {
            return Err(internal("invalid_helper_downstream"));
        }
        if crate::covered_tasks::covered_by_baseline(plan, id) {
            let obligation = plan
                .obligations
                .iter()
                .find(|obligation| obligation.task_id == *id)
                .ok_or_else(|| internal("helper_downstream_coverage_missing"))?;
            if obligation.job_id != source.job_id || order(&source.stack_id, id)? <= source_order {
                return Err(internal("helper_downstream_coverage_mismatch"));
            }
        } else {
            selected.push(id.as_str());
        }
    }
    if selected != expected {
        return Err(internal("helper_downstream_sequence_mismatch"));
    }
    Ok(())
}

/// Matrix entries contain the selected suffix of the compiled crate-job sequence.
fn expected<'a>(plan: &'a Plan, source: &MatrixEntry) -> Result<Vec<&'a str>, OrchestratorError> {
    let source_order = order(&source.stack_id, &source.task_id)?;
    let mut ranked = Vec::new();
    let mut found_source = false;
    for entry in &plan.matrix.include {
        if entry.job_id != source.job_id {
            continue;
        }
        if entry.stack_id != source.stack_id {
            return Err(internal("helper_downstream_stack_mismatch"));
        }
        let entry_order = order(&entry.stack_id, &entry.task_id)?;
        if entry.task_id == source.task_id {
            found_source = true;
        } else if entry_order > source_order
            && !crate::covered_tasks::covered_by_baseline(plan, &entry.task_id)
        {
            ranked.push(entry_order);
        }
    }
    if !found_source {
        return Err(internal("helper_downstream_source_missing"));
    }
    ranked.sort_unstable();
    Ok(ranked.into_iter().map(|(_, id)| id).collect())
}

/// Task identity provides kind; rank comes directly from the workflow generator.
fn order<'a>(stack: &str, id: &'a str) -> Result<(u32, &'a str), OrchestratorError> {
    if crate::extension_schemas::task_stack_segment(id) != Some(stack) {
        return Err(internal("helper_downstream_stack_mismatch"));
    }
    let kind = crate::extension_schemas::task_kind_segment(id)
        .ok_or_else(|| internal("helper_downstream_kind_missing"))?;
    Ok((crate::crate_jobs::obligation_kind_rank(stack, kind), id))
}

#[cfg(test)]
#[path = "helper_downstream_tests.rs"]
mod tests;
