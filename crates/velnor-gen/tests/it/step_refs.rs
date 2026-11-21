// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeSet, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use serde_json::{Value, json};

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view, workflow};

const SHARED_STORE_ON: &str = "mbx_shared_store = true\n";
const CACHE_DEBUG_ON: &str = "cache_debug = true\n";

struct StepRef<'a> {
    id: &'a str,
    reference: &'a str,
}

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

fn patches_in(text: &str) -> Vec<u64> {
    text.split("(0, 1, ")
        .skip(1)
        .filter_map(|rest| rest.split_once(')')?.0.parse().ok())
        .collect()
}

fn gate_patches() -> Result<BTreeSet<u64>, Box<dyn Error>> {
    let mut files = Vec::new();
    for krate in ["velnor-gen", "velnor-rust", "velnor-render", "velnor-model"] {
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

fn gate_patch(key: &str) -> Result<u64, Box<dyn Error>> {
    velnor_gen::key_gates()
        .find(|gate| gate.key == key && gate.value.is_none())
        .map(|gate| gate.first.2)
        .ok_or_else(|| format!("the {key} gate is missing").into())
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

fn dangling_refs(workflow: &Value) -> (usize, Vec<String>) {
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

fn shape(text: &str, helper: &str) -> String {
    text.lines()
        .filter(|line| !line.starts_with("# velnor-gen-sha256:"))
        .map(|line| line.replace(helper, "HELPER"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sweep_job(
    patch: u64,
    config: Option<&str>,
    seen: &Mutex<HashSet<String>>,
) -> Result<(usize, Vec<String>), Box<dyn Error>> {
    let helper = format!("0.1.{patch}");
    let mut repo = view(
        &["Cargo.toml", "tools/Cargo.toml"],
        config.map(str::as_bytes),
    )?;
    pin_helper(&mut repo, &helper)?;
    let text = workflow(&repo)?;
    if !seen
        .lock()
        .map_err(|_| "the render set is poisoned")?
        .insert(shape(&text, &helper))
    {
        return Ok((0, Vec::new()));
    }
    let (checked, dangling) = dangling_refs(&crate::yaml_tree::document(&text)?);
    let failures = dangling
        .into_iter()
        .map(|found| format!("helper {helper}, config {config:?}: {found}"))
        .collect();
    Ok((checked, failures))
}

#[test]
fn every_steps_reference_names_a_step_of_its_own_job() -> TestResult {
    let patches = gate_patches()?;
    let shared_store = gate_patch("mbx_shared_store")?;
    let cache_debug = gate_patch("cache_debug")?;
    assert!(
        patches.contains(&velnor_gen::HELPER_MINIMUM.2)
            && patches.contains(&shared_store)
            && patches.contains(&cache_debug),
        "gate scan found {patches:?}, which misses the minimum, shared-store or cache_debug gate"
    );
    let mut work: Vec<(u64, Option<&str>)> = Vec::new();
    for patch in patches {
        work.push((patch, None));
        if patch >= shared_store {
            work.push((patch, Some(SHARED_STORE_ON)));
        }
        if patch >= cache_debug {
            work.push((patch, Some(CACHE_DEBUG_ON)));
        }
    }
    let seen = Mutex::new(HashSet::new());
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4);
    let results: Vec<Result<(usize, Vec<String>), String>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut refs = 0;
                    let mut failures = Vec::new();
                    while let Some(&(patch, config)) =
                        work.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        let (checked, found) =
                            sweep_job(patch, config, &seen).map_err(|error| error.to_string())?;
                        refs += checked;
                        failures.extend(found);
                    }
                    Ok((refs, failures))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("a sweep worker panicked".to_owned()))
            })
            .collect()
    });
    let mut refs = 0;
    let mut failures = Vec::new();
    for result in results {
        let (checked, found) = result?;
        refs += checked;
        failures.extend(found);
    }
    failures.sort();
    assert!(failures.is_empty(), "{failures:#?}");
    assert_ne!(refs, 0, "no steps reference was checked");
    Ok(())
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
