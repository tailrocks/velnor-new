// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{StepKind, ToolsLocalVerb};
use velnor_render::{MAX_ENV_CHARACTERS, Rule, rendered_workflow};

use super::{realized_needs, required_bound};
use crate::facts::TestResult;
use crate::needs_size_plan::{saving_facts_pinned, test_packages_of};

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
        let (actual, bound, _) = check_at(roots, "0.1.110")?;
        assert!(
            actual <= bound,
            "roots={roots}, actual={actual}, bound={bound}"
        );
    }
    Ok(())
}
