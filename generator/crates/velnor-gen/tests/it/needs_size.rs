// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Map, Value, json};
use velnor_model::{RepoPath, StepKind, WorkflowPlan};
use velnor_render::{MAX_ENV_CHARACTERS, Rule};

use crate::facts::TestResult;
use crate::pipeline::{HELPER_LOCK, view};

fn saving_plan(roots: usize, blocks: bool) -> Result<WorkflowPlan, Box<dyn std::error::Error>> {
    let manifests: Vec<_> = (0..roots)
        .map(|index| format!("root_{index:02}_{}/Cargo.toml", "x".repeat(150)))
        .collect();

    let mut view = view(
        &manifests.iter().map(String::as_str).collect::<Vec<_>>(),
        Some(format!("shape = \"lint-test\"\ncache_check_blocks = {blocks}\n").as_bytes()),
    )?;
    let lock = String::from_utf8(HELPER_LOCK.to_vec())?.replacen("0.1.79", "0.1.81", 1);
    view.files
        .insert(RepoPath::new(".velnor/rust-ci.lock")?, lock.into_bytes());
    Ok(velnor_rust::plan(&velnor_rust::detect(&view)?))
}

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

fn inspection_workflow(plan: &WorkflowPlan) -> Result<Value, Box<dyn std::error::Error>> {
    let mut plan = plan.clone();
    for step in plan.jobs.iter_mut().flat_map(|job| &mut job.steps) {
        if let StepKind::Verdict {
            needs_max_bytes, ..
        }
        | StepKind::CheckMetrics { needs_max_bytes } = &mut step.kind
        {
            *needs_max_bytes = 0;
        }
    }
    let rendered = velnor_render::render(&plan).map_err(|errors| format!("{errors:?}"))?;
    Ok(serde_saphyr::from_str(
        rendered.values().next().ok_or("workflow")?,
    )?)
}

fn cache_bound(plan: &WorkflowPlan) -> Result<usize, &'static str> {
    plan.jobs
        .iter()
        .flat_map(|job| &job.steps)
        .find_map(|step| match step.kind {
            StepKind::CheckMetrics { needs_max_bytes } => Some(needs_max_bytes),
            _ => None,
        })
        .ok_or("no Cache bound")
}

#[test]
fn cache_context_is_bounded_from_its_producers_without_unrelated_plan_outputs() -> TestResult {
    for (roots, rejected) in [(2, false), (50, false), (62, true)] {
        let plan = saving_plan(roots, false)?;
        let actual = realized_needs(&inspection_workflow(&plan)?, "cache")?;
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
        let producers: Vec<_> = plan
            .jobs
            .iter()
            .filter(|job| {
                job.steps
                    .iter()
                    .any(|step| matches!(step.kind, StepKind::ReportCaches { .. }))
            })
            .map(|job| job.id.clone())
            .collect();
        assert_eq!(cache.needs, producers);
        let errors = velnor_render::render(&plan).err().unwrap_or_default();
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
    let payload_growth = realized_needs(&inspection_workflow(&blocking)?, "required")?
        - realized_needs(&inspection_workflow(&advisory)?, "required")?;
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

fn realized_output(name: &str, text: &str) -> String {
    if text.starts_with("velnor-") {
        return text
            .replace("${{ runner.os }}", "Windows")
            .replace("${{ runner.arch }}", "ARM64")
            .replace("${{ steps.prepare.outputs.compat }}", &"f".repeat(16));
    }
    if matches!(name, "tests_run" | "tests_listed") {
        return u64::MAX.to_string();
    }
    if name.ends_with("_package_args") {
        "--workspace".to_owned()
    } else if name.ends_with("_tests") {
        "run".to_owned()
    } else if name.ends_with("_mode") {
        "all".to_owned()
    } else if name.ends_with("_test_filter") {
        String::new()
    } else if name.ends_with("_docs") || name.ends_with("_doctests") {
        "run".to_owned()
    } else {
        "false".to_owned()
    }
}

fn realized_needs(workflow: &Value, consumer: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let mut needs = Map::new();
    for id in workflow["jobs"][consumer]["needs"]
        .as_array()
        .ok_or("needs")?
    {
        let id = id.as_str().ok_or("job id")?;
        let mut outputs = Map::new();
        if let Some(declared) = workflow["jobs"][id]["outputs"].as_object() {
            for (name, text) in declared {
                let value = realized_output(name, text.as_str().ok_or("output")?);
                assert!(!value.contains("${{"), "{name}: {value}");
                outputs.insert(name.clone(), Value::String(value));
            }
        }
        needs.insert(
            id.to_owned(),
            json!({"result":"cancelled","outputs":outputs}),
        );
    }
    Ok(serde_json::to_string_pretty(&needs)?.len())
}

#[test]
fn saving_lane_outputs_fit_the_required_context_bound_and_reject_an_oversized_context() -> TestResult
{
    for roots in [34, 35, 38] {
        let plan = saving_plan(roots, false)?;
        let actual = realized_needs(&inspection_workflow(&plan)?, "required")?;
        let bound = required_bound(&plan)?;
        assert!(
            actual <= bound,
            "roots={roots}, actual={actual}, bound={bound}"
        );
        if roots <= 34 {
            assert!(velnor_render::render(&plan).is_ok());
        } else {
            if roots == 38 {
                assert!(
                    actual > MAX_ENV_CHARACTERS,
                    "actual={actual}, bound={bound}"
                );
            }
            let errors = velnor_render::render(&plan)
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
