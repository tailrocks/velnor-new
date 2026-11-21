// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use serde_json::{Value, json};
use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, RepoView};
use velnor_render::DELTA_CLEANUP_PATH;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view};

const SHARED_STORE_ON: &str = "mbx_shared_store = true\n";
const SHARED_STORE_OFF: &str = "mbx_shared_store = false\n";
const CACHE_DEBUG_ON: &str = "cache_debug = true\n";
const TEST_GROUPS_ON: &str = "[[test_groups]]\nname = 'database'\nroot = '.'\nfilter = 'package(package-0) and test(/^db_/)'\n\n[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\n";
const ARTIFACTS_ON: &str = "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\n[[test_groups.artifacts]]\nname = 'shell-logs'\npaths = ['target/debug/app']\nwhen = 'always'\n";
const RESTORE_CACHE_ON: &str = "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\nneeds = ['cargo']\nrestore_cache = true\n";
const PARTITIONS_ON: &str = "[[test_groups]]\nname = 'database'\nroot = '.'\nfilter = 'package(package-0) and test(/^db_/)'\npartitions = 2\n";
const HOSTED_ON: &str = "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\nrunner = 'ubuntu-26.04'\nhosted = true\n";
const NO_TESTS_ON: &str = "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\nno_tests = 'warn'\n";
const DELTA_CLEANUP_HELPER: u64 = 102;
const SWEEP_CONFIGS: &[(&str, &str)] = &[
    ("mbx_shared_store", SHARED_STORE_ON),
    ("mbx_shared_store", SHARED_STORE_OFF),
    ("cache_debug", CACHE_DEBUG_ON),
    ("test_groups", TEST_GROUPS_ON),
    ("test_groups.artifacts", ARTIFACTS_ON),
    ("test_groups.partitions", PARTITIONS_ON),
    ("test_groups.restore_cache", RESTORE_CACHE_ON),
    ("test_groups.hosted", HOSTED_ON),
    ("test_groups.no_tests", NO_TESTS_ON),
];

fn documents(repo: &RepoView) -> Result<BTreeMap<RepoPath, String>, Box<dyn Error>> {
    let mut locked = repo.clone();
    crate::facts::lock_pinned_tools(&mut locked)?;
    Ok(expected_with_notes(&locked)?.0)
}

pub(crate) struct StepRef<'a> {
    pub(crate) id: &'a str,
    pub(crate) reference: &'a str,
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

pub(crate) fn gate_patches() -> Result<BTreeSet<u64>, Box<dyn Error>> {
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

pub(crate) fn gate_patch(key: &str) -> Result<u64, Box<dyn Error>> {
    velnor_gen::key_gates()
        .find(|gate| gate.key == key && gate.value.is_none())
        .map(|gate| gate.first.2)
        .ok_or_else(|| format!("the {key} gate is missing").into())
}

pub(crate) fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|field| strings(field, out)),
        _ => {}
    }
}

pub(crate) fn expressions(text: &str) -> Vec<&str> {
    text.split("${{")
        .skip(1)
        .filter_map(|rest| rest.split_once("}}").map(|(expr, _)| expr))
        .collect()
}

pub(crate) fn step_refs(expr: &str) -> Vec<StepRef<'_>> {
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
    let files = documents(&repo)?;
    let mut failures = Vec::new();
    if patch >= DELTA_CLEANUP_HELPER && !files.contains_key(&DELTA_CLEANUP_PATH) {
        failures.push(format!(
            "helper {helper}, config {config:?}: no cache-cleanup workflow was rendered"
        ));
    }
    let yaml: Vec<(&RepoPath, &String)> = files
        .iter()
        .filter(|(path, _)| {
            Path::new(path.as_str())
                .extension()
                .is_some_and(|ext| ext == "yml")
        })
        .collect();
    let shapes: Vec<String> = yaml.iter().map(|(_, text)| shape(text, &helper)).collect();
    if !seen
        .lock()
        .map_err(|_| "the render set is poisoned")?
        .insert(shapes.join("\n"))
    {
        return Ok((0, failures));
    }
    let mut checked = 0;
    for (path, text) in yaml {
        let (count, dangling) = dangling_refs(&crate::yaml_tree::document(text)?);
        checked += count;
        failures.extend(dangling.into_iter().map(|found| {
            format!(
                "helper {helper}, config {config:?}, {}: {found}",
                path.as_str()
            )
        }));
    }
    Ok((checked, failures))
}

#[test]
fn every_steps_reference_names_a_step_of_its_own_job() -> TestResult {
    let patches = gate_patches()?;
    let mut thresholds = Vec::new();
    for (key, config) in SWEEP_CONFIGS {
        thresholds.push((gate_patch(key)?, *config));
    }
    assert!(
        patches.contains(&velnor_gen::HELPER_MINIMUM.2)
            && thresholds.iter().all(|(gate, _)| patches.contains(gate)),
        "gate scan found {patches:?}, which misses a sweep gate in {SWEEP_CONFIGS:?}"
    );
    let mut work: Vec<(u64, Option<&str>)> = Vec::new();
    for patch in patches {
        work.push((patch, None));
        for (gate, config) in &thresholds {
            if patch >= *gate {
                work.push((patch, Some(config)));
            }
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
