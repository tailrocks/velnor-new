// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const BEFORE_SHARED_STORE: &[&str] = &["0.1.81", "0.1.84"];
const HELPERS: &[&str] = &[
    "0.1.85", "0.1.86", "0.1.87", "0.1.88", "0.1.89", "0.1.91", "0.1.92", "0.1.93", "0.1.94",
    "0.1.95", "0.1.96", "0.1.97", "0.1.98", "0.1.99", "0.1.100", "0.1.101", "0.1.102", "0.1.103",
    "0.1.104", "0.1.106", "0.1.107", "0.1.109", "0.1.110", "0.1.111",
];
const CONFIGS: &[Option<&str>] = &[
    None,
    Some("mbx_shared_store = true\n"),
    Some("mbx_shared_store = false\n"),
];

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

fn step_refs(expr: &str) -> Vec<&str> {
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
            (id_len > 0 && rest[id_len..].starts_with('.')).then(|| &rest[..id_len])
        })
        .collect()
}

fn dangling_refs(workflow: &Value) -> Vec<String> {
    let mut dangling = Vec::new();
    let Some(jobs) = workflow["jobs"].as_object() else {
        return dangling;
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
                for id in step_refs(expr) {
                    if !ids.contains(id) {
                        dangling.push(format!("{job_id}: steps.{id} in `{expr}`"));
                    }
                }
            }
        }
    }
    dangling.sort();
    dangling.dedup();
    dangling
}

#[test]
fn every_steps_reference_names_a_step_of_its_own_job() -> TestResult {
    let mut refs = 0;
    let pins = [(BEFORE_SHARED_STORE, &CONFIGS[..1]), (HELPERS, CONFIGS)];
    for (helpers, configs) in pins {
        for helper in helpers {
            for config in configs.iter().copied() {
                let mut repo = view(
                    &["Cargo.toml", "tools/Cargo.toml"],
                    config.map(str::as_bytes),
                )?;
                pin_helper(&mut repo, helper)?;
                let yaml = crate::yaml_tree::document(&workflow(&repo)?)?;
                let mut texts = Vec::new();
                strings(&yaml, &mut texts);
                refs += texts
                    .iter()
                    .flat_map(|text| expressions(text))
                    .map(|expr| step_refs(expr).len())
                    .sum::<usize>();
                let dangling = dangling_refs(&yaml);
                assert!(
                    dangling.is_empty(),
                    "helper {helper}, config {config:?}: {dangling:#?}"
                );
            }
        }
    }
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
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
