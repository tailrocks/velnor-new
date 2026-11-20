// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::{Map, Value};
use velnor_model::{PushAfterMergeGroup, StepKind, WorkflowPlan};
use velnor_render::{MAX_ENV_CHARACTERS, Rule, rendered_workflow};

use crate::facts::TestResult;
use crate::needs_size_plan::{saving_facts, saving_plan, test_packages};

mod realized;

use realized::{realized_needs, workflow_jobs};

fn required_bound(plan: &WorkflowPlan) -> Result<usize, &'static str> {
    plan.jobs
        .iter()
        .flat_map(|job| &job.steps)
        .find_map(|step| match step.kind {
            StepKind::Verdict {
                needs_max_bytes, ..
            } => Some(needs_max_bytes),
            _ => None,
        })
        .ok_or("no Required bound")
}

fn cache_bound(plan: &WorkflowPlan) -> Result<usize, &'static str> {
    plan.jobs
        .iter()
        .flat_map(|job| &job.steps)
        .find_map(|step| match step.kind {
            StepKind::CheckMetrics {
                needs_max_bytes, ..
            } => Some(needs_max_bytes),
            _ => None,
        })
        .ok_or("no Cache bound")
}

#[test]
fn cache_context_is_bounded_from_its_producers_without_unrelated_plan_outputs() -> TestResult {
    for (roots, rejected) in [(2, false), (28, false), (38, true)] {
        let plan = saving_plan(roots, false)?;
        let workflow = rendered_workflow(&plan);
        let actual = realized_needs(&workflow, "cache", &test_packages(roots, false)?)?;
        let bound = cache_bound(&plan)?;
        assert!(
            actual <= bound,
            "roots={roots}, actual={actual}, bound={bound}"
        );
        let cache = plan
            .jobs
            .iter()
            .find(|job| job.id.as_str() == "cache")
            .ok_or("Cache")?;
        let mut producers: Vec<_> = plan
            .jobs
            .iter()
            .filter(|job| {
                job.steps
                    .iter()
                    .any(|step| matches!(step.kind, StepKind::ReportCaches { .. }))
            })
            .map(|job| job.id.clone())
            .collect();
        producers.extend(
            plan.jobs
                .iter()
                .filter(|job| job.id.as_str() == "rust-trim")
                .map(|job| job.id.clone()),
        );
        assert_eq!(cache.needs, producers);
        let errors = velnor_render::check_rules(&workflow)
            .err()
            .unwrap_or_default();
        assert_eq!(
            errors
                .iter()
                .any(|error| error.job == "cache" && matches!(error.rule, Rule::EnvTooLong(_))),
            rejected,
            "roots={roots}, actual={actual}, bound={bound}, {errors:?}"
        );
        if rejected {
            assert!(actual > MAX_ENV_CHARACTERS, "actual={actual}");
        }
    }
    Ok(())
}

#[test]
fn blocking_cache_adds_its_outputless_context_to_required_without_changing_the_cache_budget()
-> TestResult {
    let advisory = saving_plan(2, false)?;
    let blocking = saving_plan(2, true)?;
    let payload_growth = realized_needs(
        &rendered_workflow(&blocking),
        "required",
        &test_packages(2, true)?,
    )? - realized_needs(
        &rendered_workflow(&advisory),
        "required",
        &test_packages(2, false)?,
    )?;
    let budget_growth = required_bound(&blocking)? - required_bound(&advisory)?;
    assert!(payload_growth > 0);
    assert!(
        budget_growth >= payload_growth,
        "payload={payload_growth}, bound={budget_growth}"
    );
    assert_eq!(cache_bound(&advisory)?, cache_bound(&blocking)?);
    for (plan, expected) in [(&advisory, false), (&blocking, true)] {
        let required = plan
            .jobs
            .iter()
            .find(|job| job.id.as_str() == "required")
            .ok_or("Required")?;
        assert_eq!(
            required.needs.iter().any(|id| id.as_str() == "cache"),
            expected
        );
        let spec = required
            .steps
            .iter()
            .find_map(|step| match &step.kind {
                StepKind::Verdict { spec, .. } => Some(spec),
                _ => None,
            })
            .ok_or("verdict spec")?;
        assert_eq!(
            spec.jobs.iter().any(|job| job.id.as_str() == "cache"),
            expected
        );
    }
    Ok(())
}

#[test]
fn the_skip_push_output_of_the_plan_job_is_inside_the_required_bound() -> TestResult {
    let mut facts = saving_facts(2, false)?;
    let without = velnor_rust::plan(&facts);
    facts.config.push_after_merge_group = PushAfterMergeGroup::SkipWhenQueueRunPassed;
    let with = velnor_rust::plan(&facts);
    let before = realized_needs(
        &rendered_workflow(&without),
        "required",
        &test_packages(2, false)?,
    )?;
    let after = realized_needs(
        &rendered_workflow(&with),
        "required",
        &test_packages(2, false)?,
    )?;
    let growth = after - before;
    assert!(growth > 0, "the plan job reports nothing more");
    assert!(
        required_bound(&with)? - required_bound(&without)? >= growth,
        "growth={growth}"
    );
    Ok(())
}

#[test]
fn production_job_lowering_preserves_planned_jobs_and_matches_yaml_dependencies_and_outputs()
-> TestResult {
    for blocks in [false, true] {
        let plan = saving_plan(2, blocks)?;
        let lowered = rendered_workflow(&plan);
        let jobs = workflow_jobs(&lowered);
        let mut plan_ids = BTreeSet::new();
        for job in &plan.jobs {
            assert!(plan_ids.insert(job.id.as_str()), "{}", job.id);
        }
        assert_eq!(plan_ids, jobs.keys().copied().collect());
        let rendered = velnor_render::render(&plan).map_err(|errors| format!("{errors:?}"))?;
        let yaml: Value = serde_saphyr::from_str(rendered.values().next().ok_or("workflow")?)?;
        let wire_jobs = yaml["jobs"].as_object().ok_or("jobs")?;
        assert_eq!(
            plan_ids,
            wire_jobs
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
        );
        for (id, job) in jobs {
            let wire = wire_jobs.get(id).ok_or("wire job")?;
            let wire_needs = match wire.get("needs") {
                Some(value) => value
                    .as_array()
                    .ok_or("wire needs")?
                    .iter()
                    .map(|value| value.as_str().ok_or("wire dependency"))
                    .collect::<Result<Vec<_>, _>>()?,
                None => Vec::new(),
            };
            assert_eq!(
                job.needs.iter().map(String::as_str).collect::<Vec<_>>(),
                wire_needs,
                "{id}"
            );
            let outputs: Map<String, Value> = job
                .outputs
                .iter()
                .map(|(name, value)| (name.clone(), Value::String(value.clone())))
                .collect();
            let wire_outputs = match wire.get("outputs") {
                Some(value) => value.as_object().ok_or("wire outputs")?.clone(),
                None => Map::new(),
            };
            assert_eq!(outputs, wire_outputs, "{id}");
        }
    }
    Ok(())
}

#[test]
fn saving_lane_outputs_fit_the_required_context_bound_and_reject_an_oversized_context() -> TestResult
{
    for roots in [22, 25, 26, 28] {
        let plan = saving_plan(roots, false)?;
        let workflow = rendered_workflow(&plan);
        let actual = realized_needs(&workflow, "required", &test_packages(roots, false)?)?;
        let bound = required_bound(&plan)?;
        assert!(
            actual <= bound,
            "roots={roots}, actual={actual}, bound={bound}"
        );
        if roots <= 25 {
            assert!(velnor_render::check_rules(&workflow).is_ok());
        } else {
            if roots == 28 {
                assert!(
                    actual > MAX_ENV_CHARACTERS,
                    "actual={actual}, bound={bound}"
                );
            }
            let errors = velnor_render::check_rules(&workflow)
                .err()
                .ok_or("oversized context accepted")?;
            assert!(
                errors
                    .iter()
                    .any(|error| error.job == "required"
                        && matches!(error.rule, Rule::EnvTooLong(_))),
                "{errors:?}"
            );
        }
    }
    Ok(())
}
