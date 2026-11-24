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

pub(crate) struct Layout {
    pub(crate) roots: Vec<String>,
    pub(crate) config: String,
    locked: bool,
    lanes: usize,
    pub(crate) helper: &'static str,
}

fn two_roots() -> Layout {
    Layout {
        roots: vec!["Cargo.toml".to_owned(), "svc/Cargo.toml".to_owned()],
        config: SECTION.to_owned(),
        locked: true,
        lanes: 2,
        helper: "0.1.115",
    }
}

fn grouped_roots() -> Layout {
    grouped_roots_at("0.1.115")
}

pub(crate) fn grouped_roots_at(helper: &'static str) -> Layout {
    Layout {
        roots: ["Cargo.toml", "tools/a/Cargo.toml", "tools/b/Cargo.toml"]
            .map(str::to_owned)
            .to_vec(),
        config: format!(
            "[[workspace_groups]]\nname = 'tools'\nroots = ['tools/a', 'tools/b']\n{SECTION}"
        ),
        locked: false,
        lanes: 2,
        helper,
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
        helper: "0.1.115",
    }
}

pub(crate) fn quoted_long_roots(helper: &'static str) -> Layout {
    let names = ["it's", "with space", "caf\u{e9}", "plain"];
    let folders: Vec<String> = (0..30)
        .map(|index| {
            let name = names[index % names.len()];
            format!("crates/a-member-with-a-rather-long-name-{index:02}/{name}")
        })
        .collect();
    let quoted: Vec<String> = folders
        .iter()
        .map(|folder| format!("\"{folder}\""))
        .collect();
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
        helper,
    }
}

fn lock(repository: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"{repository}\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/{repository}?branch=main#0123456789abcdef\"\n"
    )
}

#[derive(Clone, Copy)]
pub(crate) struct Run {
    event: &'static str,
    private: Option<&'static str>,
    only: Option<&'static str>,
}

pub(crate) const PUSH: Run = Run {
    event: "push",
    private: Some("true"),
    only: None,
};
pub(crate) const PULL_REQUEST: Run = Run {
    event: "pull_request",
    private: Some("true"),
    only: None,
};
const PUBLIC_PUSH: Run = Run {
    event: "push",
    private: Some("false"),
    only: None,
};
pub(crate) const RELEASE: Run = Run {
    event: "release",
    private: Some("true"),
    only: None,
};
const UNCHECKED_PUSH: Run = Run {
    event: "push",
    private: None,
    only: None,
};

pub(crate) fn selecting(run: Run, slug: &'static str) -> Run {
    Run {
        only: Some(slug),
        ..run
    }
}

fn context(run: Run) -> impl Fn(&str) -> Result<Value, String> {
    let Run {
        event,
        private,
        only,
    } = run;
    move |name: &str| {
        let text = |value: &str| Ok(Value::Str(value.to_owned()));
        match name {
            "github.event_name" => text(event),
            "github.ref" if event != "pull_request" => text("refs/heads/main"),
            "github.ref" => text("refs/pull/7/merge"),
            "github.event.repository.default_branch" => text("main"),
            "github.repository" | "github.event.pull_request.head.repo.full_name" => text("o/r"),
            "github.actor" | "github.event.pull_request.user.login" => text("alice"),
            "github.event.pull_request.state" => text("open"),
            "steps.private_repo.outputs.private" => {
                Ok(private.map_or(Value::Null, |value| Value::Str(value.to_owned())))
            }
            "steps.base.outputs.sources" => text("true"),
            _ if name.ends_with("_mode") => text(
                if only.is_none_or(|slug| name.ends_with(&format!("rust_{slug}_mode"))) {
                    "all"
                } else {
                    "none"
                },
            ),
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

fn manifests(step: &Json, run: Run) -> Result<BTreeSet<String>, String> {
    if let Some(list) = step["env"]["VELNOR_FETCH_MANIFESTS"].as_str().filter(|_| {
        step["run"]
            .as_str()
            .is_some_and(|line| line.contains("--manifests-env"))
    }) {
        return listed(list, run);
    }
    let line = step["run"].as_str().unwrap_or_default();
    let expanded: Vec<&str> = line
        .split_whitespace()
        .map(|word| {
            word.strip_prefix('$')
                .and_then(|name| step["env"][name].as_str())
                .unwrap_or(word)
        })
        .collect();
    Ok(expanded
        .join(" ")
        .split("--manifest-path ")
        .skip(1)
        .filter_map(|rest| rest.split_whitespace().next())
        .map(str::to_owned)
        .collect())
}

fn listed(text: &str, run: Run) -> Result<BTreeSet<String>, String> {
    let mut json = String::new();
    let mut rest = text;
    while let Some((before, after)) = rest.split_once("${{") {
        let (expression, tail) = after.split_once("}}").ok_or("unclosed expression")?;
        json.push_str(before);
        json.push_str(if evaluate(expression, &context(run))?.truthy() {
            "true"
        } else {
            "false"
        });
        rest = tail;
    }
    json.push_str(rest);
    let pairs: Vec<(String, bool)> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    pairs
        .into_iter()
        .filter(|(_, fetch)| *fetch)
        .map(|(hex, _)| {
            let bytes: Result<Vec<u8>, _> = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16))
                .collect();
            String::from_utf8(bytes.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
        })
        .collect()
}

pub(crate) fn lanes(layout: &Layout) -> Result<Vec<Vec<Json>>, Box<dyn std::error::Error>> {
    let names: Vec<&str> = layout.roots.iter().map(String::as_str).collect();
    let mut repo = private_repo(&names, &layout.config, layout.helper)?;
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

pub(crate) fn fetched(steps: &[Json], run: Run) -> Result<Vec<BTreeSet<String>>, String> {
    let mut fetched = Vec::new();
    for step in steps {
        if step["run"]
            .as_str()
            .is_some_and(|line| line.contains(FETCH))
            && runs(step, run)?
        {
            fetched.push(manifests(step, run)?);
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

pub(crate) fn every_root_on_a_saving_run(layout: &Layout) -> TestResult {
    every_root_on(layout, PUSH)
}

pub(crate) fn every_root_on(layout: &Layout, run: Run) -> TestResult {
    let every: BTreeSet<String> = layout.roots.iter().cloned().collect();
    for steps in lanes(layout)? {
        let fill = fill(&steps)?;
        assert!(runs(fill, run)?, "the fill does not run: {fill}");
        let fetched = fetched(&steps, run)?;
        assert_eq!(
            fetched,
            vec![every.clone()],
            "one credentialed fetch must hold every root the offline fill reads"
        );
    }
    Ok(())
}

pub(crate) fn own_roots(layout: &Layout, run: Run) -> TestResult {
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
