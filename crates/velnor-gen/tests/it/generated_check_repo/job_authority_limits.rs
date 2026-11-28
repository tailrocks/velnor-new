// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fmt::Write as _;

use serde_json::Value;
use velnor_gen::{GenerateError, JobAuthorityError, expected_with_notes};
use velnor_model::{JOB_AUTHORITY_INVENTORY_PATH, JobAuthorityLimit as Limit, RepoView};

use super::job_authority_inventory::enabled;
use crate::facts::TestResult;

struct Spec {
    file: String,
    branches: Vec<String>,
    pull_request_branches: Vec<String>,
    jobs: Vec<(String, Vec<String>)>,
}

impl Spec {
    fn one(file: &str) -> Self {
        Self {
            file: file.to_owned(),
            branches: vec!["main".to_owned()],
            pull_request_branches: Vec::new(),
            jobs: vec![("job".to_owned(), Vec::new())],
        }
    }
}

fn list(values: &[String]) -> String {
    let quoted: Vec<String> = values.iter().map(|value| format!("\"{value}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

fn view(specs: &[Spec]) -> Result<RepoView, Box<dyn Error>> {
    let mut toml = String::new();
    for spec in specs {
        let file = &spec.file;
        write!(
            toml,
            "\n[[workflow]]\nfile = \"{file}\"\nname = \"{file}\"\n\n[workflow.on.push]\nbranches = {}\n",
            list(&spec.branches)
        )?;
        if !spec.pull_request_branches.is_empty() {
            write!(
                toml,
                "\n[workflow.on.pull_request]\ntypes = [\"opened\", \"synchronize\"]\nbranches = {}\n",
                list(&spec.pull_request_branches)
            )?;
        }
        for (id, needs) in &spec.jobs {
            write!(
                toml,
                "\n[[workflow.job]]\nid = \"{id}\"\nneeds = {}\npermissions = {{ contents = \"read\" }}\n\n[[workflow.job.step]]\nname = \"Run\"\nrun = \"echo ok\"\n",
                list(needs)
            )?;
        }
    }
    let paths: Vec<String> = specs
        .iter()
        .map(|spec| format!(".github/workflows/{}.yml", spec.file))
        .collect();
    let listed: Vec<&str> = paths.iter().map(String::as_str).collect();
    enabled("full_run = \"off\"\n", Some(&toml), &listed)
}

fn at_and_past(
    limit: Limit,
    measure: fn(&Value) -> usize,
    build: impl Fn(usize) -> Result<RepoView, Box<dyn Error>>,
) -> TestResult {
    let edge = limit.max();
    let (files, _) = expected_with_notes(&build(edge)?)?;
    let inventory: Value = serde_json::from_str(&files[&JOB_AUTHORITY_INVENTORY_PATH])?;
    assert_eq!(measure(&inventory), edge, "{limit:?}");
    let error = expected_with_notes(&build(edge + 1)?)
        .err()
        .ok_or("one past the limit was accepted")?;
    assert!(
        matches!(
            &error,
            GenerateError::JobAuthority(JobAuthorityError::OverLimit { limit: got, listed, .. })
                if *got == limit && *listed == edge + 1
        ),
        "{limit:?}: {error:?}"
    );
    Ok(())
}

fn names(prefix: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("{prefix}{index:04}"))
        .collect()
}

fn workflows(inventory: &Value) -> &[Value] {
    inventory["workflows"].as_array().map_or(&[], Vec::as_slice)
}

fn jobs(inventory: &Value) -> Vec<&Value> {
    workflows(inventory)
        .iter()
        .flat_map(|workflow| workflow["jobs"].as_array().into_iter().flatten())
        .collect()
}

fn longest(values: impl Iterator<Item = usize>) -> usize {
    values.max().unwrap_or(0)
}

fn len(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn context_lists(inventory: &Value) -> Vec<&Value> {
    jobs(inventory)
        .into_iter()
        .flat_map(|job| job["contexts"].as_array().into_iter().flatten())
        .flat_map(|context| {
            [
                &context["branch_filters"],
                &context["pull_request_base_branches"],
            ]
        })
        .collect()
}

#[test]
fn the_workflow_limit_is_reached_by_the_generator_and_one_past_it_is_refused() -> TestResult {
    at_and_past(
        Limit::Workflows,
        |inventory| workflows(inventory).len(),
        |count| {
            let specs: Vec<Spec> = names("w", count)
                .iter()
                .map(|file| Spec::one(file))
                .collect();
            view(&specs)
        },
    )
}

#[test]
fn the_needs_limit_is_reached_by_the_generator_and_one_past_it_is_refused() -> TestResult {
    at_and_past(
        Limit::Needs,
        |inventory| longest(jobs(inventory).iter().map(|job| len(&job["needs"]))),
        |count| {
            let mut spec = Spec::one("gated");
            let earlier = names("j", count);
            spec.jobs = earlier.iter().map(|id| (id.clone(), Vec::new())).collect();
            spec.jobs.push(("last".to_owned(), earlier));
            view(&[spec])
        },
    )
}

#[test]
fn the_ref_limit_is_reached_by_the_generator_and_one_past_it_is_refused() -> TestResult {
    let refs = |inventory: &Value| longest(context_lists(inventory).into_iter().map(len));
    at_and_past(Limit::Refs, refs, |count| {
        let mut spec = Spec::one("gated");
        spec.branches = names("b", count);
        view(&[spec])
    })?;
    at_and_past(Limit::Refs, refs, |count| {
        let mut spec = Spec::one("gated");
        spec.pull_request_branches = names("b", count);
        view(&[spec])
    })
}

#[test]
fn the_job_key_limit_is_reached_by_the_generator_and_one_past_it_is_refused() -> TestResult {
    at_and_past(
        Limit::JobKeyBytes,
        |inventory| {
            longest(
                jobs(inventory)
                    .iter()
                    .map(|job| job["job_key"].as_str().map_or(0, str::len)),
            )
        },
        |bytes| {
            let mut spec = Spec::one("gated");
            spec.jobs = vec![("k".repeat(bytes), Vec::new())];
            view(&[spec])
        },
    )
}

#[test]
fn the_path_and_ref_text_limit_is_reached_by_the_generator_and_one_past_it_is_refused() -> TestResult
{
    at_and_past(
        Limit::TextBytes,
        |inventory| {
            longest(
                workflows(inventory)
                    .iter()
                    .map(|workflow| workflow["path"].as_str().map_or(0, str::len)),
            )
        },
        |bytes| {
            let overhead = ".github/workflows/.yml".len();
            view(&[Spec::one(&"f".repeat(bytes - overhead))])
        },
    )?;
    at_and_past(
        Limit::TextBytes,
        |inventory| {
            longest(
                context_lists(inventory)
                    .into_iter()
                    .flat_map(|list| list.as_array().into_iter().flatten())
                    .map(|branch| "refs/heads/".len() + branch.as_str().map_or(0, str::len)),
            )
        },
        |bytes| {
            let mut spec = Spec::one("gated");
            spec.branches = vec!["b".repeat(bytes - "refs/heads/".len())];
            view(&[spec])
        },
    )
}

#[test]
fn the_limits_the_generator_cannot_reach_hold_by_construction() -> TestResult {
    let mut spec = Spec::one("gated");
    spec.pull_request_branches = vec!["main".to_owned()];
    let (files, _) = expected_with_notes(&view(&[spec])?)?;
    let inventory: Value = serde_json::from_str(&files[&JOB_AUTHORITY_INVENTORY_PATH])?;
    for workflow in workflows(&inventory) {
        assert_eq!(workflow["calls"], serde_json::json!([]));
    }
    for job in jobs(&inventory) {
        assert_eq!(job["secrets"], serde_json::json!([]));
        assert_eq!(job["environments"], serde_json::json!([]));
        assert!(len(&job["contexts"]) <= Limit::Contexts.max());
    }
    Ok(())
}
