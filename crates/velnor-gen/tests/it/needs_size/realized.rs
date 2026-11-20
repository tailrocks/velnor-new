// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use velnor_render::{RenderedJob, RenderedWorkflow};

use crate::facts::TestResult;
use crate::needs_size_plan::{Packages, worst_package_counts};

fn realized_output(
    name: &str,
    text: &str,
    job: &RenderedJob,
    packages: &Packages,
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
            .replace("${{ github.run_id }}", &u64::MAX.to_string())
            .replace("${{ steps.sources_locks.outputs.hash }}", &"f".repeat(64));
        while let Some(start) = key.find("${{ hashFiles(") {
            let end = key[start..].find(" }}").ok_or("hash expression")? + start + 3;
            key.replace_range(start..end, &"f".repeat(64));
        }
        return realized_output("base_prefix", &key, job, packages);
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
    if name == "tests_by_package" {
        assert_eq!(expression, "steps.test.outputs.tests_by_package");
        return worst_package_counts(packages.get(&job.id).ok_or("test packages")?);
    }
    if name == "skip_push" {
        assert_eq!(expression, "steps.merge_group.outputs.skip");
        return Ok("false".to_owned());
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

pub(super) fn workflow_jobs(workflow: &RenderedWorkflow) -> BTreeMap<&str, &RenderedJob> {
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

pub(super) fn realized_needs(
    workflow: &RenderedWorkflow,
    consumer: &str,
    packages: &Packages,
) -> Result<usize, Box<dyn std::error::Error>> {
    let jobs = workflow_jobs(workflow);
    let mut needs = Map::new();
    let dependencies = &jobs.get(consumer).ok_or("consumer job")?.needs;
    assert!(!dependencies.is_empty(), "{consumer}: missing needs");
    for id in dependencies {
        let mut outputs = Map::new();
        for (name, text) in &jobs.get(id.as_str()).ok_or("producer job")?.outputs {
            let value = realized_output(
                name,
                text,
                jobs.get(id.as_str()).ok_or("producer job")?,
                packages,
            )?;
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
