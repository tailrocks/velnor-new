// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const CONFIGS: &[Option<&str>] = &[
    None,
    Some("mbx_shared_store = true\n"),
    Some("mbx_shared_store = false\n"),
];

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_sources(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// The patch number of every `(0, 1, N)` tuple in `text`, in order.
fn patches_in(text: &str) -> Vec<u64> {
    text.split("(0, 1, ")
        .skip(1)
        .filter_map(|rest| rest.split_once(')')?.0.parse().ok())
        .collect()
}

/// Every helper patch that gates a workflow shape: each `(0, 1, N)` gate tuple in the generator,
/// planner and renderer sources, so a new gate is covered without editing this test.
fn gate_patches() -> Result<BTreeSet<u64>, Box<dyn Error>> {
    let mut files = Vec::new();
    for krate in ["velnor-gen", "velnor-rust", "velnor-render"] {
        let src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(krate)
            .join("src");
        rust_sources(&src, &mut files)?;
    }
    let mut patches = BTreeSet::new();
    for file in files {
        let text = fs::read_to_string(file)?;
        patches.extend(
            patches_in(&text)
                .into_iter()
                .filter(|patch| *patch >= velnor_gen::HELPER_MINIMUM.2),
        );
    }
    Ok(patches)
}

/// The first patch whose helper accepts `mbx_shared_store`, from its gate in `key_gates`.
fn shared_store_patch() -> Result<u64, Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/key_gates/rust_ci.rs");
    let text = fs::read_to_string(path)?;
    let (_, after) = text
        .split_once("\"mbx_shared_store\"")
        .ok_or("the mbx_shared_store gate is missing")?;
    patches_in(after)
        .first()
        .copied()
        .ok_or_else(|| "the mbx_shared_store gate has no threshold".into())
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
    let patches = gate_patches()?;
    let shared_store = shared_store_patch()?;
    assert!(
        patches.contains(&velnor_gen::HELPER_MINIMUM.2) && patches.contains(&shared_store),
        "gate scan found {patches:?}, which misses the minimum or the shared-store gate"
    );
    for patch in patches {
        let helper = format!("0.1.{patch}");
        let configs = if patch < shared_store {
            &CONFIGS[..1]
        } else {
            CONFIGS
        };
        for config in configs.iter().copied() {
            let mut repo = view(
                &["Cargo.toml", "tools/Cargo.toml"],
                config.map(str::as_bytes),
            )?;
            pin_helper(&mut repo, &helper)?;
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
