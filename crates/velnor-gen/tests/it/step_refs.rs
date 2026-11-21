// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, RepoView};
use velnor_render::DELTA_CLEANUP_PATH;

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view};

const SHARED_STORE_ON: &str = "mbx_shared_store = true\n";
const SHARED_STORE_OFF: &str = "mbx_shared_store = false\n";
const CACHE_DEBUG_ON: &str = "cache_debug = true\n";
const TEST_GROUPS_ON: &str = "[[test_groups]]\nname = 'database'\nroot = '.'\nfilter = 'package(package-0) and test(/^db_/)'\n\n[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\n";
const DELTA_CLEANUP_HELPER: u64 = 102;
const GROUP_SHAPES: &[(&str, &str)] = &[
    (
        "test_groups.artifacts",
        "[[test_groups]]\nname = 'database'\nroot = '.'\nfilter = 'package(package-0) and test(/^db_/)'\nartifacts = [{ name = 'database-failures', paths = ['junit.xml'], when = 'failure' }]\n",
    ),
    (
        "test_groups.partitions",
        "[[test_groups]]\nname = 'database'\nroot = '.'\nfilter = 'package(package-0) and test(/^db_/)'\npartitions = 2\n",
    ),
    (
        "test_groups.restore_cache",
        "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\nneeds = ['cargo']\nrestore_cache = true\n",
    ),
    (
        "test_groups.hosted",
        "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\nrunner = 'ubuntu-26.04'\nhosted = true\n",
    ),
    (
        "test_groups.no_tests",
        "[[test_groups]]\nname = 'shell'\nroot = '.'\nfilter = 'binary(it) and test(~cargo_)'\nno_tests = 'warn'\n",
    ),
];

fn documents(repo: &RepoView) -> Result<BTreeMap<RepoPath, String>, Box<dyn Error>> {
    let mut locked = repo.clone();
    crate::facts::lock_pinned_tools(&mut locked)?;
    Ok(expected_with_notes(&locked)?.0)
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

fn gate_patch(key: &str) -> Result<u64, Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/key_gates/rust_ci.rs");
    let text = fs::read_to_string(path)?;
    let (_, after) = text
        .split_once(&format!("\"{key}\""))
        .ok_or_else(|| format!("the {key} gate is missing"))?;
    patches_in(after)
        .first()
        .copied()
        .ok_or_else(|| format!("the {key} gate has no threshold").into())
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

pub(crate) fn step_refs(expr: &str) -> Vec<&str> {
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

pub(crate) fn dangling_refs(workflow: &Value) -> Vec<String> {
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
    let shared_store = gate_patch("mbx_shared_store")?;
    let cache_debug = gate_patch("cache_debug")?;
    let test_groups = gate_patch("test_groups")?;
    let mut shapes = Vec::new();
    for (key, config) in GROUP_SHAPES {
        shapes.push((gate_patch(key)?, *config));
    }
    assert!(
        patches.contains(&velnor_gen::HELPER_MINIMUM.2)
            && patches.contains(&shared_store)
            && patches.contains(&cache_debug)
            && patches.contains(&test_groups),
        "gate scan found {patches:?}, which misses the minimum, shared-store, cache_debug or test_groups gate"
    );
    for patch in patches {
        let helper = format!("0.1.{patch}");
        let mut configs: Vec<Option<&str>> = vec![None];
        if patch >= shared_store {
            configs.extend([Some(SHARED_STORE_ON), Some(SHARED_STORE_OFF)]);
        }
        if patch >= cache_debug {
            configs.push(Some(CACHE_DEBUG_ON));
        }
        if patch >= test_groups {
            configs.push(Some(TEST_GROUPS_ON));
        }
        for (gate, config) in &shapes {
            if patch >= *gate {
                configs.push(Some(*config));
            }
        }
        for config in configs {
            let mut repo = view(
                &["Cargo.toml", "tools/Cargo.toml"],
                config.map(str::as_bytes),
            )?;
            pin_helper(&mut repo, &helper)?;
            let files = documents(&repo)?;
            if patch >= DELTA_CLEANUP_HELPER {
                assert!(
                    files.contains_key(&DELTA_CLEANUP_PATH),
                    "helper {helper}, config {config:?}: no cache-cleanup workflow was rendered"
                );
            }
            for (path, text) in &files {
                if Path::new(path.as_str())
                    .extension()
                    .is_none_or(|ext| ext != "yml")
                {
                    continue;
                }
                let yaml = crate::yaml_tree::document(text)?;
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
                    "helper {helper}, config {config:?}, {}: {dangling:#?}",
                    path.as_str()
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
