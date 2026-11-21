// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{StepKind, ToolsLocalVerb};
use velnor_render::{MAX_ENV_CHARACTERS, Rule, rendered_workflow};

use super::{cache_bound, realized_needs, required_bound};
use crate::facts::TestResult;
use crate::needs_size_plan::{saving_facts_pinned, saving_facts_shaped, test_packages_of};

fn check(roots: usize) -> Result<(usize, usize, bool), Box<dyn std::error::Error>> {
    check_at(roots, "0.1.111")
}

fn check_at(
    roots: usize,
    helper: &str,
) -> Result<(usize, usize, bool), Box<dyn std::error::Error>> {
    let facts = saving_facts_pinned(roots, false, Some(helper))?;
    let plan = velnor_rust::plan(&facts);
    assert!(
        plan.jobs
            .iter()
            .flat_map(|job| &job.steps)
            .any(|step| matches!(
                step.kind,
                StepKind::ToolsLocal(ToolsLocalVerb::AnchorRead, _)
            )),
        "roots={roots}: no job reads the anchor"
    );
    let workflow = rendered_workflow(&plan);
    let actual = realized_needs(&workflow, "required", &test_packages_of(&facts)?)?;
    let refused = velnor_render::check_rules(&workflow)
        .err()
        .unwrap_or_default()
        .iter()
        .any(|error| error.job == "required" && matches!(error.rule, Rule::EnvTooLong(_)));
    Ok((actual, required_bound(&plan)?, refused))
}

#[test]
fn the_tools_anchor_outputs_of_the_plan_job_are_inside_the_required_bound() -> TestResult {
    for roots in [1, 2] {
        let (actual, bound, _) = check(roots)?;
        assert!(
            actual <= bound,
            "roots={roots}, actual={actual}, bound={bound}"
        );
    }
    Ok(())
}

#[test]
fn a_required_context_over_the_limit_with_the_anchor_outputs_is_refused() -> TestResult {
    let roots = 27;
    let (actual, bound, refused) = check(roots)?;
    assert!(
        actual <= bound,
        "roots={roots}, actual={actual}, bound={bound}"
    );
    assert!(
        actual > MAX_ENV_CHARACTERS,
        "roots={roots}, actual={actual}"
    );
    assert!(refused, "roots={roots}, bound={bound}");
    Ok(())
}

#[test]
fn the_tools_anchor_outputs_are_budgeted_for_a_0_1_110_helper() -> TestResult {
    for roots in [1, 2] {
        let (actual, bound, refused) = check_at(roots, "0.1.110")?;
        assert!(
            actual <= bound,
            "roots={roots}, actual={actual}, bound={bound}"
        );
        assert!(!refused, "roots={roots}, actual={actual}");
    }
    let roots = 27;
    let (actual, bound, refused) = check_at(roots, "0.1.110")?;
    assert!(
        actual <= bound,
        "roots={roots}, actual={actual}, bound={bound}"
    );
    assert!(
        actual > MAX_ENV_CHARACTERS && refused,
        "roots={roots}, actual={actual}, refused={refused}"
    );
    Ok(())
}

fn single_job_sizes(
    helper: &str,
) -> Result<(usize, usize, usize, bool), Box<dyn std::error::Error>> {
    let facts = saving_facts_shaped(1, false, Some(helper), "one-job")?;
    let plan = velnor_rust::plan(&facts);
    let workflow = rendered_workflow(&plan);
    let renders_key = workflow.jobs.iter().any(|job| {
        job.outputs
            .iter()
            .any(|(name, _)| name == "tools_anchor_key")
    });
    let actual = realized_needs(&workflow, "required", &test_packages_of(&facts)?)?;
    Ok((
        actual,
        required_bound(&plan)?,
        cache_bound(&plan)?,
        renders_key,
    ))
}

#[test]
fn a_single_job_budgets_no_anchor_key_it_does_not_render() -> TestResult {
    let (kept_actual, kept_required, kept_cache, kept_key) = single_job_sizes("0.1.111")?;
    let (actual, required, cache, key) = single_job_sizes("0.1.112")?;
    assert!(kept_key && !key);
    assert!(actual <= required, "actual={actual}, bound={required}");
    let freed = kept_actual - actual;
    assert!(freed > 0);
    assert!(
        kept_required - required >= freed,
        "required bound {kept_required} -> {required}, realized {kept_actual} -> {actual}"
    );
    assert!(
        kept_cache - cache >= freed,
        "cache bound {kept_cache} -> {cache}, realized {kept_actual} -> {actual}"
    );
    Ok(())
}
