// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use velnor_model::{RepoPath, StepKind, WorkflowPlan};
use velnor_render::{MAX_ENV_CHARACTERS, RenderedJob, RenderedWorkflow, Rule, rendered_workflow};

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
        let actual = realized_needs(&workflow, "cache")?;
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
    let payload_growth = realized_needs(&rendered_workflow(&blocking), "required")?
        - realized_needs(&rendered_workflow(&advisory), "required")?;
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

fn realized_output(
    name: &str,
    text: &str,
    job: &RenderedJob,
) -> Result<String, Box<dyn std::error::Error>> {
    if matches!(name, "target_key" | "sources_key") {
        let id = text
            .strip_prefix("${{ steps.")
            .and_then(|inner| inner.strip_suffix(".outputs.key }}"))
            .ok_or("key output")?;
        let step = job
            .steps
            .iter()
            .find(|step| step.id.as_deref() == Some(id))
            .ok_or("key step")?;
        let (_, velnor_render::Value::Expr(key)) = step
            .env
            .iter()
            .find(|(name, _)| name == "CACHE_KEY")
            .ok_or("cache key")?
        else {
            return Err("cache key expression".into());
        };
        let mut key = key
            .replace("${{ github.sha }}", &"a".repeat(40))
            .replace("${{ github.run_id }}", &u64::MAX.to_string());
        while let Some(start) = key.find("${{ hashFiles(") {
            let end = key[start..].find(" }}").ok_or("hash expression")? + start + 3;
            key.replace_range(start..end, &"f".repeat(64));
        }
        return realized_output("base_prefix", &key, job);
    }
    if matches!(name, "base_prefix" | "sources_prefix") && text.starts_with("velnor-") {
        let value = text
            .replace("${{ runner.os }}", "Windows")
            .replace("${{ runner.arch }}", "ARM64")
            .replace("${{ steps.prepare.outputs.compat }}", &"f".repeat(16));
        if value.contains("${{") {
            return Err(format!("unsupported cache expression: {name}: {text}").into());
        }
        return Ok(value);
    }
    let expression = text
        .strip_prefix("${{ ")
        .and_then(|inner| inner.strip_suffix(" }}"))
        .ok_or_else(|| format!("unsupported output expression: {name}: {text}"))?;
    if matches!(name, "tests_run" | "tests_listed") {
        assert_eq!(expression, format!("steps.test.outputs.{name}"));
        return Ok(u64::MAX.to_string());
    }
    if matches!(name, "base_save" | "sources_saved") {
        check_cache_expression(name, expression)?;
        return Ok("false".to_owned());
    }
    if !name.starts_with("rust_") || expression != format!("steps.plan.outputs.{name}") {
        return Err(format!("unsupported output: {name}: {text}").into());
    }
    let value = if name.ends_with("_package_args") {
        "--workspace".to_owned()
    } else if name.ends_with("_tests") {
        "run".to_owned()
    } else if name.ends_with("_mode") {
        "all".to_owned()
    } else if name.ends_with("_test_filter") {
        String::new()
    } else if name.ends_with("_docs") || name.ends_with("_doctests") {
        "run".to_owned()
    } else if name.ends_with("_doctests_known")
        || matches!(name, "rust_deny" | "rust_machete" | "rust_policy")
    {
        "false".to_owned()
    } else {
        return Err(format!("unsupported output: {name}: {text}").into());
    };
    Ok(value)
}

fn check_cache_expression(name: &str, expression: &str) -> TestResult {
    let supported = match name {
        "base_save" => expression == "steps.base.outputs.save",
        "sources_saved" => expression
            .strip_prefix("steps.sources_save_")
            .and_then(|inner| inner.strip_suffix(".outcome == 'success'"))
            .is_some_and(|slug| !slug.is_empty() && valid_step_id(slug)),
        _ => false,
    };
    if !supported {
        return Err(format!("unsupported cache expression: {name}: {expression}").into());
    }
    Ok(())
}

fn valid_step_id(id: &str) -> bool {
    id.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn workflow_jobs(workflow: &RenderedWorkflow) -> BTreeMap<&str, &RenderedJob> {
    let mut jobs = BTreeMap::new();
    for job in &workflow.jobs {
        assert!(jobs.insert(job.id.as_str(), job).is_none(), "{}", job.id);
        let mut names = BTreeSet::new();
        for (name, _) in &job.outputs {
            assert!(names.insert(name), "{}: {name}", job.id);
        }
    }
    jobs
}

fn realized_needs(
    workflow: &RenderedWorkflow,
    consumer: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let jobs = workflow_jobs(workflow);
    let mut needs = Map::new();
    let dependencies = &jobs.get(consumer).ok_or("consumer job")?.needs;
    assert!(!dependencies.is_empty(), "{consumer}: missing needs");
    for id in dependencies {
        let mut outputs = Map::new();
        for (name, text) in &jobs.get(id.as_str()).ok_or("producer job")?.outputs {
            let value = realized_output(name, text, jobs.get(id.as_str()).ok_or("producer job")?)?;
            assert!(!value.contains("${{"), "{name}: {value}");
            outputs.insert(name.clone(), Value::String(value));
        }
        needs.insert(
            id.to_owned(),
            json!({"result":"cancelled","outputs":outputs}),
        );
    }
    Ok(serde_json::to_string_pretty(&needs)?.len())
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
    for roots in [22, 24, 26] {
        let plan = saving_plan(roots, false)?;
        let workflow = rendered_workflow(&plan);
        let actual = realized_needs(&workflow, "required")?;
        let bound = required_bound(&plan)?;
        assert!(
            actual <= bound,
            "roots={roots}, actual={actual}, bound={bound}"
        );
        if roots <= 22 {
            assert!(velnor_render::check_rules(&workflow).is_ok());
        } else {
            if roots == 26 {
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
