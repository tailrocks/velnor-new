// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value as Json;
use velnor_test_support::gh_expr::{Value, evaluate};

use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const ROOTS: [&str; 2] = ["Cargo.toml", "svc/Cargo.toml"];
const LOCKS: [(&str, &str); 2] = [
    ("Cargo.lock", "shared-lib"),
    ("svc/Cargo.lock", "other-lib"),
];
const FETCH: &str = "\"$VELNOR_EXEC\" fetch ";
const FILL: &str = "\"$VELNOR_EXEC\" sources fetch --roots";

fn lock(repository: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"{repository}\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/{repository}?branch=main#0123456789abcdef\"\n"
    )
}

fn context(event: &'static str) -> impl Fn(&str) -> Result<Value, String> {
    move |name: &str| {
        let text = |value: &str| Ok(Value::Str(value.to_owned()));
        match name {
            "github.event_name" => text(event),
            "github.ref" if event == "push" => text("refs/heads/main"),
            "github.ref" => text("refs/pull/7/merge"),
            "github.event.repository.default_branch" => text("main"),
            "github.repository" | "github.event.pull_request.head.repo.full_name" => text("o/r"),
            "github.actor" | "github.event.pull_request.user.login" => text("alice"),
            "github.event.pull_request.state" => text("open"),
            "steps.private_repo.outputs.private" | "steps.base.outputs.sources" => text("true"),
            _ if name.ends_with("_mode") => text("all"),
            _ if name.ends_with(".outcome") || name.ends_with(".result") => text("success"),
            _ => Ok(Value::Null),
        }
    }
}

fn runs(step: &Json, event: &'static str) -> Result<bool, String> {
    step["if"].as_str().map_or(Ok(true), |guard| {
        Ok(evaluate(guard, &context(event))?.truthy())
    })
}

fn manifests(run: &str) -> BTreeSet<String> {
    run.split("--manifest-path ")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

fn lanes() -> Result<Vec<Vec<Json>>, Box<dyn std::error::Error>> {
    let mut repo = private_repo(&ROOTS, SECTION, "0.1.115")?;
    for (file, repository) in LOCKS {
        repo.files
            .insert(path(file)?, lock(repository).into_bytes());
    }
    let lanes: Vec<Vec<Json>> = jobs(&workflow(&repo)?)?
        .into_iter()
        .filter_map(|job| job["steps"].as_array().cloned())
        .filter(|steps| {
            steps
                .iter()
                .any(|step| step["run"].as_str().is_some_and(|run| run.contains(FILL)))
        })
        .collect();
    assert_eq!(lanes.len(), ROOTS.len(), "one saving lane per root");
    Ok(lanes)
}

fn fetched(steps: &[Json], event: &'static str) -> Result<Vec<BTreeSet<String>>, String> {
    let mut fetched = Vec::new();
    for step in steps {
        let Some(run) = step["run"].as_str().filter(|run| run.contains(FETCH)) else {
            continue;
        };
        if runs(step, event)? {
            fetched.push(manifests(run));
        }
    }
    Ok(fetched)
}

#[test]
fn a_saving_run_fetches_with_the_token_every_root_the_offline_sources_fill_lists() -> TestResult {
    let every: BTreeSet<String> = ROOTS.iter().map(|root| (*root).to_owned()).collect();
    for steps in lanes()? {
        let fill = steps
            .iter()
            .find(|step| step["run"].as_str().is_some_and(|run| run.contains(FILL)))
            .ok_or("no fill")?;
        assert!(runs(fill, "push")?, "the fill does not run: {fill}");
        let fetched = fetched(&steps, "push")?;
        assert_eq!(
            fetched,
            vec![every.clone()],
            "one credentialed fetch must hold every root the offline fill reads"
        );
    }
    Ok(())
}

#[test]
fn a_pull_request_fetches_with_the_token_only_the_roots_its_job_builds() -> TestResult {
    let mut seen = BTreeSet::new();
    for steps in lanes()? {
        let fetched = fetched(&steps, "pull_request")?;
        assert_eq!(fetched.len(), 1, "{fetched:?}");
        assert_eq!(fetched[0].len(), 1, "{fetched:?}");
        seen.extend(fetched[0].iter().cloned());
    }
    assert_eq!(seen.len(), ROOTS.len(), "{seen:?}");
    Ok(())
}
