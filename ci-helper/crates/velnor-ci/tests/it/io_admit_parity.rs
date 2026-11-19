// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `admit-preview` against the legacy "Admit source preview" script, on one recorded
//! GitHub: the same verdict, the same requests in the same order, and the same files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use velnor_ci::io::preview::admit::publish;

use crate::admit_cases::all;
use crate::admit_world::{AdmitWorld, world_json};
use crate::preview_admit::{helper, tree};
use crate::support::fresh_dir;

fn renderer() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../crates/velnor-actions-workflow-renderer/src")
}

fn python_ready() -> bool {
    Command::new("python3")
        .args(["-I", "-S", "-c", "import tomllib"])
        .output()
        .is_ok_and(|out| out.status.success())
}

fn directory(name: &str) -> PathBuf {
    fs::canonicalize(fresh_dir("velnor-ci-admit", name)).unwrap_or_else(|e| unreachable!("{e}"))
}

struct Outcome {
    ok: bool,
    calls: Vec<String>,
    files: std::collections::BTreeMap<String, (Vec<u8>, u32)>,
    output: String,
}

fn outcome(root: &Path, ok: bool, calls: Vec<String>) -> Outcome {
    Outcome {
        ok,
        calls,
        files: tree(&root.join("temp")),
        output: fs::read_to_string(root.join("output")).unwrap_or_default(),
    }
}

fn temp_with(base: &Path, name: &str, existing: &[&str]) -> PathBuf {
    let root = base.join(name);
    fs::create_dir_all(root.join("temp")).unwrap_or_else(|e| unreachable!("{e}"));
    for file in existing {
        fs::write(root.join("temp").join(file), b"mine").unwrap_or_else(|e| unreachable!("{e}"));
    }
    root
}

/// A case: a name, a world and the files that already exist in `RUNNER_TEMP`.
type Case<'a> = (String, AdmitWorld, &'a [&'a str]);

/// Run the legacy script on every case in one interpreter, each with its own environment.
fn legacy(base: &Path, cases: &[Case<'_>]) -> Vec<Outcome> {
    let roots: Vec<PathBuf> = cases
        .iter()
        .map(|(name, _, existing)| temp_with(base, &format!("py-{name}"), existing))
        .collect();
    let list: Vec<Value> = cases
        .iter()
        .zip(&roots)
        .map(|((_, world, _), root)| {
            let mut value = world_json(world);
            let mut env = world.vars.clone();
            env.insert(
                "RUNNER_TEMP".to_owned(),
                root.join("temp").to_string_lossy().into_owned(),
            );
            env.insert(
                "GITHUB_OUTPUT".to_owned(),
                root.join("output").to_string_lossy().into_owned(),
            );
            value["env"] = json!(env);
            value
        })
        .collect();
    let file = base.join("cases.json");
    fs::write(&file, json!({"cases": list}).to_string()).unwrap_or_else(|e| unreachable!("{e}"));
    let harness =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/preview_parity/admit.py");
    let out = Command::new("python3")
        .args(["-I", "-S"])
        .arg(harness)
        .arg(renderer())
        .arg(&file)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&out.stdout);
    let values: Vec<Value> = serde_json::from_str(text.trim())
        .unwrap_or_else(|e| unreachable!("{e}: {text} {}", String::from_utf8_lossy(&out.stderr)));
    values
        .iter()
        .zip(&roots)
        .map(|(value, root)| {
            let calls = value["calls"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c.as_str().map(str::to_owned))
                .collect();
            outcome(root, value["ok"] == true, calls)
        })
        .collect()
}

fn current(base: &Path, world: &AdmitWorld, name: &str, existing: &[&str]) -> Outcome {
    let root = temp_with(base, &format!("rs-{name}"), existing);
    let (result, calls) = helper(world);
    let ok = result
        .and_then(|admission| publish(&root.join("temp"), &root.join("output"), &admission))
        .is_ok();
    outcome(&root, ok, calls)
}

fn same(old: &Outcome, new: &Outcome, name: &str) {
    assert_eq!(old.ok, new.ok, "verdict: {name}");
    assert_eq!(old.calls, new.calls, "requests: {name}");
    assert_eq!(old.files, new.files, "files: {name}");
    assert_eq!(old.output, new.output, "output: {name}");
}

fn one(world: &AdmitWorld, name: &str, existing: &[&str]) -> (Outcome, Outcome) {
    let case = (name.to_owned(), world.clone(), existing);
    let base = directory(name);
    let old = legacy(&base, &[case])
        .pop()
        .unwrap_or_else(|| unreachable!("one case"));
    (old, current(&base, world, name, existing))
}

#[test]
fn preview_parity_admit_happy_path_matches_the_legacy_script() {
    if !python_ready() {
        eprintln!("skipped: python3 with tomllib is not available");
        return;
    }
    let (old, new) = one(&AdmitWorld::new(), "ok", &[]);
    assert!(old.ok && new.ok);
    assert_eq!(old.files.len(), 3);
    same(&old, &new, "happy path");
}

#[test]
fn preview_parity_admit_refusals_match_the_legacy_script() {
    if !python_ready() {
        eprintln!("skipped: python3 with tomllib is not available");
        return;
    }
    let cases: Vec<Case<'_>> = all()
        .into_iter()
        .enumerate()
        .map(|(index, (name, mutate))| {
            let mut world = AdmitWorld::new();
            mutate(&mut world);
            (format!("{index}-{name}").replace(' ', "-"), world, &[][..])
        })
        .collect();
    let base = directory("refusals");
    let old = legacy(&base, &cases);
    for ((name, world, _), old) in cases.iter().zip(&old) {
        let new = current(&base, world, name, &[]);
        assert!(!old.ok, "the legacy script admitted: {name}");
        assert!(!new.ok, "the helper admitted: {name}");
        same(old, &new, name);
    }
}

#[test]
fn preview_parity_admit_refuses_to_replace_a_file_like_the_legacy_script() {
    if !python_ready() {
        eprintln!("skipped: python3 with tomllib is not available");
        return;
    }
    let world = AdmitWorld::new();
    let linux = ["velnor-actions-x86_64-unknown-linux-gnu"];
    let (old, new) = one(&world, "link", &linux);
    assert!(!old.ok && !new.ok);
    assert_eq!(old.files.keys().collect::<Vec<_>>(), linux);
    same(&old, &new, "payload exists");
    // The legacy script leaves the payload links it made; the helper removes them.
    let metadata = ["source-preview-metadata.json"];
    let (old, new) = one(&world, "meta", &metadata);
    assert!(!old.ok && !new.ok);
    assert_eq!(old.calls, new.calls);
    assert_eq!(new.files.keys().collect::<Vec<_>>(), metadata);
}
