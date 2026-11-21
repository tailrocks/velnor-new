// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

#[derive(Default)]
pub(crate) struct Scan {
    pub(crate) workflows: usize,
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

fn expressions(text: &str) -> Vec<&str> {
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

fn step_refs(expr: &str) -> Vec<&str> {
    let masked = unquoted(expr);
    masked
        .match_indices("steps.")
        .filter(|(at, _)| begins_a_context(&masked, *at))
        .filter_map(|(at, _)| {
            let rest = &expr[at + "steps.".len()..];
            let id_len = ident_len(rest);
            (id_len > 0 && rest[id_len..].starts_with('.')).then(|| &rest[..id_len])
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
    steps: BTreeSet<&'a str>,
    jobs: &'a Map<String, Value>,
}

fn needs_problem(site: &Site<'_>, job: &str, output: Option<&str>) -> Option<&'static str> {
    let Some(target) = site.jobs.get(job) else {
        return Some("names no job");
    };
    if !site.needs.contains(job) {
        return Some("is not a direct need");
    }
    let declared = output.is_none_or(|name| {
        target["outputs"]
            .as_object()
            .is_some_and(|outputs| outputs.contains_key(name))
    });
    (!declared).then_some("names no output of the needed job")
}

fn check(value: &Value, key: &str, site: &Site<'_>, scan: &mut Scan) {
    match value {
        Value::String(text) => {
            let exprs = if key == "if" && !text.contains("${{") {
                vec![text.as_str()]
            } else {
                expressions(text)
            };
            for expr in exprs {
                for id in step_refs(expr) {
                    scan.refs += 1;
                    if !site.steps.contains(id) {
                        scan.dangling
                            .push(format!("{}: steps.{id} in `{expr}`", site.job));
                    }
                }
                for (job, output) in needs_refs(expr) {
                    scan.refs += 1;
                    if let Some(problem) = needs_problem(site, job, output) {
                        scan.dangling
                            .push(format!("{}: needs.{job} {problem} in `{expr}`", site.job));
                    }
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| check(item, key, site, scan)),
        Value::Object(fields) => fields
            .iter()
            .for_each(|(name, field)| check(field, name, site, scan)),
        _ => {}
    }
}

pub(crate) fn scan_workflow(workflow: &Value, scan: &mut Scan) {
    let Some(jobs) = workflow["jobs"].as_object() else {
        return;
    };
    for (job_id, job) in jobs {
        let steps: Vec<&Value> = job["steps"].as_array().into_iter().flatten().collect();
        let needs: BTreeSet<&str> = match &job["needs"] {
            Value::String(name) => BTreeSet::from([name.as_str()]),
            Value::Array(names) => names.iter().filter_map(Value::as_str).collect(),
            _ => BTreeSet::new(),
        };
        if let Some(fields) = job.as_object() {
            let site = Site {
                job: job_id.as_str(),
                needs: needs.clone(),
                steps: steps
                    .iter()
                    .filter_map(|step| step["id"].as_str())
                    .collect(),
                jobs,
            };
            for (key, value) in fields.iter().filter(|(key, _)| key.as_str() != "steps") {
                check(value, key, &site, scan);
            }
        }
        for (index, step) in steps.iter().copied().enumerate() {
            let site = Site {
                job: job_id.as_str(),
                needs: needs.clone(),
                steps: steps[..index]
                    .iter()
                    .filter_map(|earlier| earlier["id"].as_str())
                    .collect(),
                jobs,
            };
            check(step, "", &site, scan);
        }
    }
}

fn dangling_refs(workflow: &Value) -> Vec<String> {
    let mut scan = Scan::default();
    scan_workflow(workflow, &mut scan);
    scan.dangling.sort();
    scan.dangling.dedup();
    scan.dangling
}

#[test]
fn the_reference_check_reads_only_expressions_and_only_step_ids() {
    assert_eq!(
        step_refs("steps.toolsanchor.outputs.cache-hit == 'true' && x.steps.nope.y"),
        vec!["toolsanchor"]
    );
    assert_eq!(step_refs("steps.a-b_1.outputs.x"), vec!["a-b_1"]);
    assert_eq!(step_refs("steps.toolsanchor"), Vec::<&str>::new());
    assert_eq!(
        expressions("run $x steps.a.b ${{ steps.c.outputs.d }} tail"),
        vec![" steps.c.outputs.d "]
    );
}

#[test]
fn a_step_name_inside_a_quoted_string_is_not_a_reference() {
    assert_eq!(
        step_refs("format('steps.nope.x {0}', y)"),
        Vec::<&str>::new()
    );
}

#[test]
fn a_closing_delimiter_inside_a_quoted_string_does_not_end_the_expression() {
    assert_eq!(
        expressions("${{ format('{{literal}} {0}', steps.missing.outputs.x) }} tail"),
        vec![" format('{{literal}} {0}', steps.missing.outputs.x) "]
    );
}

#[test]
fn a_step_reference_to_a_step_that_has_not_run_yet_is_dangling() {
    let workflow = json!({"jobs": {"build": {"steps": [
        {"id": "first", "if": "${{ steps.second.outcome == 'success' }}", "run": "true"},
        {"id": "second", "run": "true"}
    ]}}});
    assert!(
        !dangling_refs(&workflow).is_empty(),
        "{:?}",
        dangling_refs(&workflow)
    );
}

#[test]
fn a_bare_if_is_read_as_an_expression() {
    let workflow = json!({"jobs": {"build": {"steps": [
        {"id": "first", "run": "true"},
        {"if": "steps.missing.outcome == 'success'", "run": "true"}
    ]}}});
    assert_ne!(dangling_refs(&workflow), Vec::<String>::new());
}

#[test]
fn a_job_output_may_reference_a_step_of_its_job() {
    let workflow = json!({"jobs": {"build": {
        "outputs": {"y": "${{ steps.second.outputs.y }}"},
        "steps": [{"id": "first", "run": "true"}, {"id": "second", "run": "true"}]
    }}});
    assert_eq!(dangling_refs(&workflow), Vec::<String>::new());
}

#[test]
fn a_needs_output_of_a_direct_need_is_clean() {
    let workflow = json!({"jobs": {
        "build": {"outputs": {"key": "k"}, "steps": [{"run": "true"}]},
        "deploy": {"needs": ["build"], "steps": [{"run": "echo ${{ needs.build.outputs.key }}"}]}
    }});
    assert_eq!(dangling_refs(&workflow), Vec::<String>::new());
}

#[test]
fn a_needs_output_that_the_needed_job_does_not_declare_is_dangling() {
    let workflow = json!({"jobs": {
        "build": {"outputs": {"key": "k"}, "steps": [{"run": "true"}]},
        "deploy": {"needs": ["build"], "steps": [{"run": "echo ${{ needs.build.outputs.missing }}"}]}
    }});
    assert_ne!(dangling_refs(&workflow), Vec::<String>::new());
}

#[test]
fn a_needs_reference_to_a_job_that_is_not_a_direct_need_is_dangling() {
    let workflow = json!({"jobs": {
        "build": {"outputs": {"key": "k"}, "steps": [{"run": "true"}]},
        "deploy": {"needs": [], "steps": [{"run": "echo ${{ needs.build.result }}"}]}
    }});
    assert_ne!(dangling_refs(&workflow), Vec::<String>::new());
}
