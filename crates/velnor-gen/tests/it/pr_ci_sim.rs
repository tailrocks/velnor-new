// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::Value as Json;

use crate::gh_expr::{Value, evaluate, has_status_function};

#[derive(Clone)]
pub(crate) struct Event {
    pub(crate) name: &'static str,
    pub(crate) git_ref: &'static str,
    pub(crate) pull_request: Option<Pull>,
}

#[derive(Clone)]
pub(crate) struct Pull {
    pub(crate) draft: bool,
    pub(crate) labels: Vec<&'static str>,
    pub(crate) same_repository: bool,
    pub(crate) action: &'static str,
    pub(crate) label: &'static str,
    pub(crate) base_changed: bool,
}

pub(crate) fn pull(draft: bool, labels: &[&'static str]) -> Event {
    Event {
        name: "pull_request",
        git_ref: "refs/pull/7/merge",
        pull_request: Some(Pull {
            draft,
            labels: labels.to_vec(),
            same_repository: true,
            action: "synchronize",
            label: "",
            base_changed: false,
        }),
    }
}

pub(crate) fn edited(base_changed: bool, labels: &[&'static str]) -> Event {
    let mut event = pull(false, labels);
    if let Some(pull) = event.pull_request.as_mut() {
        pull.action = "edited";
        pull.base_changed = base_changed;
    }
    event
}

pub(crate) fn converted_to_draft(labels: &[&'static str]) -> Event {
    let mut event = pull(true, labels);
    if let Some(pull) = event.pull_request.as_mut() {
        pull.action = "converted_to_draft";
    }
    event
}

pub(crate) fn labeled(added: &'static str, labels: &[&'static str]) -> Event {
    let mut event = pull(false, labels);
    if let Some(pull) = event.pull_request.as_mut() {
        pull.action = "labeled";
        pull.label = added;
    }
    event
}

pub(crate) fn fork_pull(labels: &[&'static str]) -> Event {
    let mut event = pull(false, labels);
    if let Some(pull) = event.pull_request.as_mut() {
        pull.same_repository = false;
    }
    event
}

pub(crate) fn other(name: &'static str, git_ref: &'static str) -> Event {
    Event {
        name,
        git_ref,
        pull_request: None,
    }
}

pub(crate) fn context(
    event: &Event,
    results: &BTreeMap<String, String>,
    needs: &[String],
) -> impl Fn(&str) -> Result<Value, String> {
    let event = event.clone();
    let results = results.clone();
    let needs = needs.to_vec();
    move |path: &str| {
        let pull = event.pull_request.as_ref();
        let text = |value: &str| Value::Str(value.to_owned());
        let value = match path {
            "github.event_name" => text(event.name),
            "github.ref" => text(event.git_ref),
            "github.repository" => text("owner/repo"),
            "github.run_id" => text("99"),
            "github.workflow_ref" => {
                text("owner/repo/.github/workflows/rust-ci.yml@refs/pull/7/merge")
            }
            "github.event.pull_request.number" => pull.map_or(Value::Null, |_| text("7")),
            "github.event.action" => pull.map_or(Value::Null, |pull| text(pull.action)),
            "github.event.changes.base" => pull
                .filter(|pull| pull.action == "edited" && pull.base_changed)
                .map_or(Value::Null, |_| text("main")),
            "github.event.label.name" => pull
                .filter(|pull| pull.action == "labeled")
                .map_or(Value::Null, |pull| text(pull.label)),
            "github.event.repository.default_branch" => text("main"),
            "github.event.pull_request.state" => pull.map_or(Value::Null, |_| text("open")),
            "github.event.pull_request.draft" => {
                pull.map_or(Value::Null, |pull| Value::Bool(pull.draft))
            }
            "github.event.pull_request.head.repo.full_name" => pull.map_or(Value::Null, |pull| {
                text(if pull.same_repository {
                    "owner/repo"
                } else {
                    "fork/repo"
                })
            }),
            "github.event.pull_request.labels.*.name" => Value::List(
                pull.map(|pull| pull.labels.iter().map(|label| text(label)).collect())
                    .unwrap_or_default(),
            ),
            "needs.*.result" => Value::List(
                needs
                    .iter()
                    .filter_map(|need| results.get(need))
                    .map(|result| text(result))
                    .collect(),
            ),
            other => {
                let parts: Vec<&str> = other.split('.').collect();
                match parts.as_slice() {
                    ["needs", job, "result"] => results.get(*job).map_or(Value::Null, |r| text(r)),
                    ["needs", _, "outputs", key] if key.ends_with("_mode") => text("all"),
                    ["needs", _, "outputs", _] => text("run"),
                    _ => return Err(format!("the simulation does not know {other}")),
                }
            }
        };
        Ok(value)
    }
}

pub(crate) fn strings(value: Option<&Json>) -> Vec<String> {
    value
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn simulate(
    tree: &Json,
    event: &Event,
    failing: &[&str],
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let jobs = tree
        .get("jobs")
        .and_then(Json::as_object)
        .ok_or("no jobs")?;
    let mut results: BTreeMap<String, String> = BTreeMap::new();
    while results.len() < jobs.len() {
        let before = results.len();
        for (id, job) in jobs {
            let needs = strings(job.get("needs"));
            if results.contains_key(id) || needs.iter().any(|need| !results.contains_key(need)) {
                continue;
            }
            let condition = job.get("if").and_then(Json::as_str);
            let implicit = condition.is_none_or(|text| !has_status_function(text))
                && needs.iter().any(|need| results[need] != "success");
            let holds = match condition {
                _ if implicit => false,
                None => true,
                Some(text) => evaluate(text, &context(event, &results, &needs))?.truthy(),
            };
            let result = match (holds, failing.contains(&id.as_str())) {
                (false, _) => "skipped",
                (true, true) => "failure",
                (true, false) => "success",
            };
            results.insert(id.clone(), result.to_owned());
        }
        assert!(
            results.len() > before,
            "the jobs need each other in a cycle"
        );
    }
    Ok(results)
}

pub(crate) struct Verdict {
    pub(crate) steps: Vec<String>,
    pub(crate) passed: bool,
}

pub(crate) fn required(
    tree: &Json,
    event: &Event,
    results: &BTreeMap<String, String>,
) -> Result<Verdict, Box<dyn std::error::Error>> {
    let job = tree.pointer("/jobs/required").ok_or("no required job")?;
    let needs = strings(job.get("needs"));
    let resolve = context(event, results, &needs);
    let mut verdict = Verdict {
        steps: Vec::new(),
        passed: true,
    };
    for step in job
        .get("steps")
        .and_then(Json::as_array)
        .ok_or("no steps")?
    {
        let condition = step.get("if").and_then(Json::as_str);
        let holds = match condition {
            None => verdict.passed,
            Some(text) => {
                (verdict.passed || has_status_function(text)) && evaluate(text, &resolve)?.truthy()
            }
        };
        if holds {
            let name = step
                .get("name")
                .and_then(Json::as_str)
                .ok_or("a step has no name")?;
            verdict.steps.push(name.to_owned());
            verdict.passed &= step.get("run").and_then(Json::as_str) != Some("exit 1");
        }
    }
    Ok(verdict)
}

pub(crate) fn outcome(
    tree: &Json,
    event: &Event,
    failing: &[&str],
) -> Result<(BTreeMap<String, String>, Verdict), Box<dyn std::error::Error>> {
    let results = simulate(tree, event, failing)?;
    let verdict = required(tree, event, &results)?;
    Ok((results, verdict))
}

pub(crate) fn state<'a>(results: &'a BTreeMap<String, String>, job: &str) -> &'a str {
    results.get(job).map_or("missing", String::as_str)
}
