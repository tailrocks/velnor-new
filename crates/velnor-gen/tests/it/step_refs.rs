// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

pub(crate) type Callees = BTreeMap<String, BTreeSet<String>>;

pub(crate) struct StepRef<'a> {
    pub(crate) id: &'a str,
    pub(crate) reference: &'a str,
}

#[derive(Default)]
pub(crate) struct Scan {
    pub(crate) refs: usize,
    pub(crate) dangling: Vec<String>,
}

fn ident_len(text: &str) -> usize {
    text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(text.len())
}

fn unquoted(expr: &str) -> String {
    let mut masked = String::with_capacity(expr.len());
    let mut quoted = false;
    for ch in expr.chars() {
        if ch == '\'' {
            quoted = !quoted;
            masked.push(' ');
        } else if quoted {
            masked.extend(std::iter::repeat_n(' ', ch.len_utf8()));
        } else {
            masked.push(ch);
        }
    }
    masked
}

fn begins_a_context(masked: &str, at: usize) -> bool {
    masked[..at]
        .chars()
        .last()
        .is_none_or(|before| !(before.is_alphanumeric() || before == '_' || before == '.'))
}

pub(crate) fn expressions(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("${{") {
        let body = &rest[start + "${{".len()..];
        let end = unquoted(body).find("}}").unwrap_or(body.len());
        found.push(&body[..end]);
        rest = &body[(end + "}}".len()).min(body.len())..];
    }
    found
}

pub(crate) fn step_refs(expr: &str) -> Vec<StepRef<'_>> {
    let masked = unquoted(expr);
    masked
        .match_indices("steps.")
        .filter(|(at, _)| begins_a_context(&masked, *at))
        .filter_map(|(at, _)| {
            let rest = &expr[at + "steps.".len()..];
            let id_len = ident_len(rest);
            (id_len > 0).then(|| {
                let reference_len = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
                    .unwrap_or(rest.len());
                StepRef {
                    id: &rest[..id_len],
                    reference: &expr[at..at + "steps.".len() + reference_len],
                }
            })
        })
        .collect()
}

fn needs_refs(expr: &str) -> Vec<(&str, Option<&str>)> {
    let masked = unquoted(expr);
    masked
        .match_indices("needs.")
        .filter(|(at, _)| begins_a_context(&masked, *at))
        .filter_map(|(at, _)| {
            let rest = &expr[at + "needs.".len()..];
            let job_len = ident_len(rest);
            (job_len > 0).then(|| {
                let output = rest[job_len..]
                    .strip_prefix(".outputs.")
                    .map(|tail| &tail[..ident_len(tail)]);
                (&rest[..job_len], output)
            })
        })
        .collect()
}

struct Site<'a> {
    job: &'a str,
    needs: BTreeSet<&'a str>,
    allowed: BTreeSet<&'a str>,
    all: BTreeSet<&'a str>,
    jobs: &'a Map<String, Value>,
    callees: &'a Callees,
}

fn needs_problem(site: &Site<'_>, job: &str, output: Option<&str>) -> Option<&'static str> {
    let Some(target) = site.jobs.get(job) else {
        return Some("names no job");
    };
    if !site.needs.contains(job) {
        return Some("is not a direct need of this job");
    }
    let declared = output.is_none_or(|name| match target["uses"].as_str() {
        Some(uses) => site
            .callees
            .get(uses)
            .is_none_or(|outputs| outputs.contains(name)),
        None => target["outputs"]
            .as_object()
            .is_some_and(|outputs| outputs.contains_key(name)),
    });
    (!declared).then_some("names no output of the needed job")
}

fn check(value: &Value, bare: bool, site: &Site<'_>, scan: &mut Scan) {
    match value {
        Value::String(text) => {
            let exprs = if bare && !text.contains("${{") {
                vec![text.as_str()]
            } else {
                expressions(text)
            };
            for expr in exprs {
                for found in step_refs(expr) {
                    scan.refs += 1;
                    if !site.allowed.contains(found.id) {
                        let problem = if site.all.contains(found.id) {
                            "names a step that has not run yet"
                        } else {
                            "names no step of this job"
                        };
                        scan.dangling.push(format!(
                            "{}: `{}` {problem} (id `{}`) in `{}`",
                            site.job,
                            found.reference,
                            found.id,
                            expr.trim()
                        ));
                    }
                }
                for (job, output) in needs_refs(expr) {
                    scan.refs += 1;
                    if let Some(problem) = needs_problem(site, job, output) {
                        scan.dangling.push(format!(
                            "{}: `needs.{job}` {problem} in `{}`",
                            site.job,
                            expr.trim()
                        ));
                    }
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| check(item, false, site, scan)),
        Value::Object(fields) => fields
            .values()
            .for_each(|field| check(field, false, site, scan)),
        _ => {}
    }
}

pub(crate) fn scan_workflow(workflow: &Value, callees: &Callees, scan: &mut Scan) {
    let Some(jobs) = workflow["jobs"].as_object() else {
        return;
    };
    for (job_id, job) in jobs {
        let Some(fields) = job.as_object() else {
            continue;
        };
        let steps: Vec<&Value> = job["steps"].as_array().into_iter().flatten().collect();
        let needs: BTreeSet<&str> = match &job["needs"] {
            Value::String(name) => BTreeSet::from([name.as_str()]),
            Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
            _ => BTreeSet::new(),
        };
        let all: BTreeSet<&str> = steps
            .iter()
            .filter_map(|step| step["id"].as_str())
            .collect();
        for (name, value) in fields.iter().filter(|(name, _)| name.as_str() != "steps") {
            let site = Site {
                job: job_id.as_str(),
                needs: needs.clone(),
                allowed: if name == "outputs" {
                    all.clone()
                } else {
                    BTreeSet::new()
                },
                all: all.clone(),
                jobs,
                callees,
            };
            check(value, name == "if", &site, scan);
        }
        for (index, step) in steps.iter().copied().enumerate() {
            let Some(step_fields) = step.as_object() else {
                continue;
            };
            let site = Site {
                job: job_id.as_str(),
                needs: needs.clone(),
                allowed: steps[..index]
                    .iter()
                    .filter_map(|earlier| earlier["id"].as_str())
                    .collect(),
                all: all.clone(),
                jobs,
                callees,
            };
            for (name, value) in step_fields {
                check(value, name == "if", &site, scan);
            }
        }
    }
}

pub(crate) fn dangling_refs(workflow: &Value, callees: &Callees) -> (usize, Vec<String>) {
    let mut scan = Scan::default();
    scan_workflow(workflow, callees, &mut scan);
    scan.dangling.sort();
    scan.dangling.dedup();
    (scan.refs, scan.dangling)
}
