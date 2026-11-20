// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::{CacheKey, RepoFacts, RepoPath, RepoView, StepKind, WorkflowPlan};
use velnor_render::{MAX_WORKFLOW_BYTES, Rule, RuleViolation, render};
use velnor_rust::{check_required_check, plan, plan_without_deltas};

use crate::GenerateError;
use crate::delta_cache;

pub(crate) type RenderedPlan = (WorkflowPlan, BTreeMap<RepoPath, String>, Vec<String>);

pub(crate) fn plan_and_render(
    view: &RepoView,
    facts: &RepoFacts,
) -> Result<RenderedPlan, GenerateError> {
    let full = plan(facts);
    delta_cache::validate_staging(view, &full)?;
    check_required_check(facts, &full)?;
    let violations = match render(&full) {
        Ok(files) => return Ok((full, files, Vec::new())),
        Err(violations) => violations,
    };
    let roots = facts.rust_roots.len();
    let Some(bytes) = oversized(&violations) else {
        return Err(GenerateError::Rules(violations));
    };
    if !has_deltas(&full) {
        return Err(GenerateError::Rules(with_roots(violations, roots)));
    }
    let reduced = plan_without_deltas(facts);
    delta_cache::validate_staging(view, &reduced)?;
    check_required_check(facts, &reduced)?;
    let files = render(&reduced)
        .map_err(|violations| GenerateError::Rules(with_roots(violations, roots)))?;
    let note = format!(
        "the delta cache layer was left out because the workflow with it would be {bytes} bytes, above the {MAX_WORKFLOW_BYTES}-byte limit GitHub sets on a workflow file"
    );
    Ok((reduced, files, vec![note]))
}

fn with_roots(mut violations: Vec<RuleViolation>, roots: usize) -> Vec<RuleViolation> {
    for violation in &mut violations {
        if matches!(violation.rule, Rule::WorkflowTooLarge(_)) {
            violation.job = format!("{}; {roots} Cargo roots", violation.job);
        }
    }
    violations
}

fn oversized(violations: &[RuleViolation]) -> Option<usize> {
    let mut bytes = None;
    for violation in violations {
        let Rule::WorkflowTooLarge(size) = &violation.rule else {
            return None;
        };
        bytes = Some(*size);
    }
    bytes
}

fn has_deltas(plan: &WorkflowPlan) -> bool {
    plan.jobs.iter().flat_map(|job| &job.steps).any(|step| {
        matches!(
            step.kind,
            StepKind::CacheRestore(CacheKey::Delta { .. })
                | StepKind::CacheSave(CacheKey::Delta { .. })
        )
    })
}
