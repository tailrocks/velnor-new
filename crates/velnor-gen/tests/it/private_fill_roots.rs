// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value as Json;
use velnor_test_support::gh_expr::{Value, evaluate};

use crate::facts::{TestResult, path};
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const LOCKS: [(&str, &str); 2] = [
    ("Cargo.lock", "shared-lib"),
    ("svc/Cargo.lock", "other-lib"),
];
const FETCH: &str = "\"$VELNOR_EXEC\" fetch ";
const FILL: &str = "\"$VELNOR_EXEC\" sources fetch --roots";

struct Layout {
    roots: Vec<String>,
    config: String,
    locked: bool,
    lanes: usize,
}

fn two_roots() -> Layout {
    Layout {
        roots: vec!["Cargo.toml".to_owned(), "svc/Cargo.toml".to_owned()],
        config: SECTION.to_owned(),
        locked: true,
        lanes: 2,
    }
}

fn grouped_roots() -> Layout {
    Layout {
        roots: ["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"]
            .map(str::to_owned)
            .to_vec(),
        config: format!(
            "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n{SECTION}"
        ),
        locked: false,
        lanes: 2,
    }
}

fn long_roots() -> Layout {
    let folders: Vec<String> = (0..30)
        .map(|index| format!("crates/a-member-with-a-rather-long-directory-name-{index:02}"))
        .collect();
    let quoted: Vec<String> = folders.iter().map(|folder| format!("'{folder}'")).collect();
    Layout {
        roots: std::iter::once("Cargo.toml".to_owned())
            .chain(folders.iter().map(|folder| format!("{folder}/Cargo.toml")))
            .collect(),
        config: format!(
            "[[workspace_groups]]\nname = 'members'\nroots = [{}]\n{SECTION}",
            quoted.join(", ")
        ),
        locked: false,
        lanes: 2,
    }
}

fn lock(repository: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"{repository}\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/{repository}?branch=main#0123456789abcdef\"\n"
    )
}

#[derive(Clone, Copy)]
struct Run {
    event: &'static str,
    private: Option<&'static str>,
}

const PUSH: Run = Run {
    event: "push",
    private: Some("true"),
};
const PULL_REQUEST: Run = Run {
    event: "pull_request",
    private: Some("true"),
};
const PUBLIC_PUSH: Run = Run {
    event: "push",
    private: Some("false"),
};
const UNCHECKED_PUSH: Run = Run {
    event: "push",
    private: None,
};

fn context(run: Run) -> impl Fn(&str) -> Result<Value, String> {
    let Run { event, private } = run;
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
            "steps.private_repo.outputs.private" => {
                Ok(private.map_or(Value::Null, |value| Value::Str(value.to_owned())))
            }
            "steps.base.outputs.sources" => text("true"),
            _ if name.ends_with("_mode") => text("all"),
            _ if name.ends_with(".outcome") || name.ends_with(".result") => text("success"),
            _ => Ok(Value::Null),
        }
    }
}

fn runs(step: &Json, run: Run) -> Result<bool, String> {
    step["if"].as_str().map_or(Ok(true), |guard| {
        Ok(evaluate(guard, &context(run))?.truthy())
    })
}

fn manifests(step: &Json) -> BTreeSet<String> {
    let run = step["run"].as_str().unwrap_or_default();
    let expanded: Vec<&str> = run
        .split_whitespace()
        .map(|word| {
            word.strip_prefix('$')
                .and_then(|name| step["env"][name].as_str())
                .unwrap_or(word)
        })
        .collect();
    expanded
        .join(" ")
        .split("--manifest-path ")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

fn lanes(layout: &Layout) -> Result<Vec<Vec<Json>>, Box<dyn std::error::Error>> {
    let names: Vec<&str> = layout.roots.iter().map(String::as_str).collect();
    let mut repo = private_repo(&names, &layout.config, "0.1.115")?;
    for (file, repository) in LOCKS.iter().filter(|_| layout.locked) {
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
    assert_eq!(lanes.len(), layout.lanes, "saving lanes");
    Ok(lanes)
}

fn fetched(steps: &[Json], run: Run) -> Result<Vec<BTreeSet<String>>, String> {
    let mut fetched = Vec::new();
    for step in steps {
        if step["run"]
            .as_str()
            .is_some_and(|line| line.contains(FETCH))
            && runs(step, run)?
        {
            fetched.push(manifests(step));
        }
    }
    Ok(fetched)
}

fn fill(steps: &[Json]) -> Result<&Json, &'static str> {
    steps
        .iter()
        .find(|step| step["run"].as_str().is_some_and(|run| run.contains(FILL)))
        .ok_or("no fill")
}

fn every_root_on_a_saving_run(layout: &Layout) -> TestResult {
    let every: BTreeSet<String> = layout.roots.iter().cloned().collect();
    for steps in lanes(layout)? {
        let fill = fill(&steps)?;
        assert!(runs(fill, PUSH)?, "the fill does not run: {fill}");
        let fetched = fetched(&steps, PUSH)?;
        assert_eq!(
            fetched,
            vec![every.clone()],
            "one credentialed fetch must hold every root the offline fill reads"
        );
    }
    Ok(())
}

fn own_roots(layout: &Layout, run: Run) -> TestResult {
    let mut seen = BTreeSet::new();
    let mut count = 0;
    for steps in lanes(layout)? {
        let fetched = fetched(&steps, run)?;
        assert_eq!(fetched.len(), 1, "{fetched:?}");
        count += fetched[0].len();
        seen.extend(fetched[0].iter().cloned());
    }
    assert_eq!(count, layout.roots.len(), "a root fetched twice: {seen:?}");
    assert_eq!(seen.len(), layout.roots.len(), "{seen:?}");
    Ok(())
}

#[test]
fn a_saving_run_fetches_with_the_token_every_root_the_offline_sources_fill_lists() -> TestResult {
    every_root_on_a_saving_run(&two_roots())
}

#[test]
fn a_pull_request_fetches_with_the_token_only_the_roots_its_job_builds() -> TestResult {
    own_roots(&two_roots(), PULL_REQUEST)
}

#[test]
fn a_saving_run_fetches_every_root_when_their_paths_outgrow_one_command_line() -> TestResult {
    every_root_on_a_saving_run(&long_roots())?;
    own_roots(&long_roots(), PULL_REQUEST)
}

#[test]
fn a_grouped_job_fetches_with_the_token_every_root_it_compiles() -> TestResult {
    own_roots(&grouped_roots(), PULL_REQUEST)?;
    every_root_on_a_saving_run(&grouped_roots())
}

#[test]
fn a_saving_run_not_confirmed_private_fetches_with_the_token_only_the_roots_its_job_builds()
-> TestResult {
    for run in [PUBLIC_PUSH, UNCHECKED_PUSH] {
        for steps in lanes(&two_roots())? {
            let fill = fill(&steps)?;
            assert!(!runs(fill, run)?, "the fill runs: {fill}");
        }
        own_roots(&two_roots(), run)?;
    }
    Ok(())
}
