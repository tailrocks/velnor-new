// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::{Value, json};

struct StepRef<'a> {
    id: &'a str,
    reference: &'a str,
}

fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|field| strings(field, out)),
        _ => {}
    }
}

fn expressions(text: &str) -> Vec<&str> {
    text.split("${{")
        .skip(1)
        .filter_map(|rest| rest.split_once("}}").map(|(expr, _)| expr))
        .collect()
}

fn step_refs(expr: &str) -> Vec<StepRef<'_>> {
    expr.match_indices("steps.")
        .filter(|(at, _)| {
            expr[..*at]
                .chars()
                .last()
                .is_none_or(|before| !(before.is_alphanumeric() || before == '_' || before == '.'))
        })
        .filter_map(|(at, _)| {
            let rest = &expr[at + "steps.".len()..];
            let id_len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(rest.len());
            let reference_len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
                .unwrap_or(rest.len());
            (id_len > 0).then(|| StepRef {
                id: &rest[..id_len],
                reference: &expr[at..at + "steps.".len() + reference_len],
            })
        })
        .collect()
}

pub(crate) fn dangling_refs(workflow: &Value) -> (usize, Vec<String>) {
    let mut checked = 0;
    let mut dangling = Vec::new();
    let Some(jobs) = workflow["jobs"].as_object() else {
        return (checked, dangling);
    };
    for (job_id, job) in jobs {
        let ids: BTreeSet<&str> = job["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|step| step["id"].as_str())
            .collect();
        let mut texts = Vec::new();
        strings(job, &mut texts);
        for text in texts {
            for expr in expressions(text) {
                for found in step_refs(expr) {
                    checked += 1;
                    if !ids.contains(found.id) {
                        dangling.push(format!(
                            "{job_id}: `{}` names no step of this job (id `{}`) in `{}`",
                            found.reference,
                            found.id,
                            expr.trim()
                        ));
                    }
                }
            }
        }
    }
    dangling.sort();
    dangling.dedup();
    (checked, dangling)
}

fn references(expr: &str) -> Vec<(&str, &str)> {
    step_refs(expr)
        .into_iter()
        .map(|found| (found.id, found.reference))
        .collect()
}

#[test]
fn the_reference_check_reads_only_expressions_and_only_step_ids() {
    assert_eq!(
        references("steps.toolsanchor.outputs.cache-hit == 'true' && x.steps.nope.y"),
        vec![("toolsanchor", "steps.toolsanchor.outputs.cache-hit")]
    );
    assert_eq!(
        references("steps.a-b_1.outputs.x"),
        vec![("a-b_1", "steps.a-b_1.outputs.x")]
    );
    assert_eq!(
        references("toJSON(steps.toolsanchor)"),
        vec![("toolsanchor", "steps.toolsanchor")]
    );
    assert_eq!(
        expressions("run $x steps.a.b ${{ steps.c.outputs.d }} tail"),
        vec![" steps.c.outputs.d "]
    );
}

#[test]
fn a_dangling_name_sharing_a_prefix_with_a_real_step_is_reported_whole() {
    let workflow = json!({"jobs": {"rust-root": {"steps": [
        {"id": "toolsanchor", "run": "true"},
        {"run": "${{ steps.toolsanchor.outputs.cache-hit }}"},
        {"run": "${{ steps.toolsanchorx.outputs.cache-hit == 'true' }}"},
        {"run": "${{ toJSON(steps.toolsanchor2) }}"},
    ]}}});
    let (checked, dangling) = dangling_refs(&workflow);
    assert_eq!(checked, 3);
    assert_eq!(
        dangling,
        vec![
            "rust-root: `steps.toolsanchor2` names no step of this job (id `toolsanchor2`) in `toJSON(steps.toolsanchor2)`",
            "rust-root: `steps.toolsanchorx.outputs.cache-hit` names no step of this job (id `toolsanchorx`) in `steps.toolsanchorx.outputs.cache-hit == 'true'`",
        ]
    );
}
