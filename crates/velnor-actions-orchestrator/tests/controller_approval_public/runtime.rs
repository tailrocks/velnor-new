// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, io};

use serde_json::Value;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::{
    BASE_APPROVALS, BASE_LOCK, fixture, lock_for_version, prepare_error, rotated_approvals,
};

const CURRENT_RUNTIME_SHA256: [&str; 3] = [
    "3162760f40b6b5f706c60641e4ef5ee7322bc6c54f72fc26a7111b337aa18d9d",
    "7dc61dfa71abbcda1c40d18440bc556b5d58b80e6042bc4f95fd106fa1bb3c66",
    "8d5e1591811143f15334f690373cefb58c13bd6cbd0292008748fd777f6c99d8",
];

const ROTATED_RUNTIME_SHA256: [&str; 3] = [
    "1111111111111111111111111111111111111111111111111111111111111111",
    "2222222222222222222222222222222222222222222222222222222222222222",
    "3333333333333333333333333333333333333333333333333333333333333333",
];

fn rendered_ci(approvals: &str, lock: &str) -> Result<String, Box<dyn Error>> {
    let temp = fixture(Some(approvals), lock)?;
    let prep = prepare(temp.path())?;
    let tree = render_staged_tree(&prep)?;
    tree.get(".github/workflows/ci.yml")
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("rendered ci workflow missing").into())
}

fn assert_runtime_identities(
    ci: &str,
    version: &str,
    hashes: [&str; 3],
) -> Result<(), Box<dyn Error>> {
    for (job_id, hash) in [
        "check-fixture-mise-linux",
        "check-fixture-mise-arm",
        "check-fixture-mise-intel",
    ]
    .into_iter()
    .zip(hashes)
    {
        let job = rendered_job_block(ci, job_id)?;
        let marker = "      - name: Setup Mise\n";
        let (_, tail) = job
            .split_once(marker)
            .ok_or_else(|| format!("Setup Mise missing from {job_id}"))?;
        if tail.contains(marker) {
            return Err(format!("ambiguous Setup Mise in {job_id}").into());
        }
        let end = tail.find("\n      - ").unwrap_or(tail.len());
        let setup = &tail[..end];
        assert!(action_scalar(setup, "uses")?.starts_with("jdx/mise-action@"));
        assert_eq!(action_scalar(setup, "version")?, version, "{job_id}");
        assert_eq!(action_scalar(setup, "sha256")?, hash, "{job_id}");
    }
    Ok(())
}

fn rendered_job_block<'a>(workflow: &'a str, job_id: &str) -> Result<&'a str, Box<dyn Error>> {
    let marker = format!("  {job_id}:\n");
    let start = workflow
        .find(&marker)
        .ok_or_else(|| format!("rendered job {job_id} is missing"))?;
    let tail = &workflow[start..];
    let mut end = tail.len();
    let mut offset = 0;
    for (index, line) in tail.split_inclusive('\n').enumerate() {
        if index > 0
            && line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
        {
            end = offset;
            break;
        }
        offset += line.len();
    }
    Ok(&tail[..end])
}

fn action_scalar<'a>(action: &'a str, name: &str) -> Result<&'a str, Box<dyn Error>> {
    let prefix = format!("{name}: ");
    let mut values = action
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix(&prefix));
    let value = values
        .next()
        .ok_or_else(|| format!("missing Setup Mise {name}"))?;
    if values.next().is_some() {
        return Err(format!("ambiguous Setup Mise {name}").into());
    }
    if let Some(quote) = value.chars().next().filter(|c| matches!(c, '\'' | '"')) {
        return value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
            .ok_or_else(|| format!("unmatched Setup Mise {name} quotes").into());
    }
    if value.ends_with(['\'', '"']) {
        return Err(format!("unmatched Setup Mise {name} quotes").into());
    }
    Ok(value)
}

fn rotated_runtime_approvals(approvals: &str) -> Result<String, Box<dyn Error>> {
    let mut value: Value = serde_json::from_str(approvals)?;
    let runtime = value
        .pointer_mut("/active/mise_runtime")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("active runtime approval missing"))?;
    for (entry, hash) in runtime.iter_mut().zip(ROTATED_RUNTIME_SHA256) {
        entry["version"] = Value::String("2026.10.22".to_owned());
        entry["sha256"] = Value::String(hash.to_owned());
    }
    Ok(serde_json::to_string(&value)?)
}

#[test]
fn current_active_runtime_renders_all_three_target_identities() -> Result<(), Box<dyn Error>> {
    let ci = rendered_ci(BASE_APPROVALS, BASE_LOCK)?;
    assert_runtime_identities(&ci, "2026.10.19", CURRENT_RUNTIME_SHA256)?;
    Ok(())
}

#[test]
fn active_release_and_runtime_rotation_changes_all_three_rendered_targets()
-> Result<(), Box<dyn Error>> {
    let commit = "dddddddddddddddddddddddddddddddddddddddd";
    let approvals = rotated_runtime_approvals(&rotated_approvals())?;
    let lock = lock_for_version("0.1.60", commit);
    let ci = rendered_ci(&approvals, &lock)?;
    assert_runtime_identities(&ci, "2026.10.22", ROTATED_RUNTIME_SHA256)?;
    for hash in CURRENT_RUNTIME_SHA256 {
        assert!(!ci.contains(hash), "old runtime identity remains: {hash}");
    }
    Ok(())
}

#[test]
fn bootstrap_rotation_accepts_only_matching_lock_without_changing_runtime_output()
-> Result<(), Box<dyn Error>> {
    let approvals =
        super::approvals_with_mise(super::ROTATED_MISE_VERSION, super::ROTATED_MISE_SHA);
    let lock =
        super::lock_with_mise_bootstrap(super::ROTATED_MISE_VERSION, super::ROTATED_MISE_SHA);
    let ci = rendered_ci(&approvals, &lock)?;
    assert_runtime_identities(&ci, "2026.10.19", CURRENT_RUNTIME_SHA256)?;

    let stale_lock = super::lock_with_mise_bootstrap(
        "2026.10.21",
        "ccfe12d7a62c6fb9def2a20db1f210f13a51de881f21fd8c85db4b6312405d07",
    );
    let temp = fixture(Some(&approvals), &stale_lock)?;
    let error = prepare_error(temp.path())?;
    assert!(error.contains("lock_mismatch:mise_bootstrap"), "{error}");
    Ok(())
}

#[test]
fn staged_runtime_rotation_is_inert_in_rendered_output() -> Result<(), Box<dyn Error>> {
    let staged = BASE_APPROVALS.replace(
        "\"staged\": null",
        &format!("\"staged\": {}", super::STAGED_ENTRY),
    );
    let ci = rendered_ci(&staged, BASE_LOCK)?;
    assert_runtime_identities(&ci, "2026.10.19", CURRENT_RUNTIME_SHA256)?;
    assert!(!ci.contains("2026.10.20"));
    assert!(!ci.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    assert!(!ci.contains("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    assert!(!ci.contains("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"));
    Ok(())
}

fn runtime_entries_mut(value: &mut Value) -> Result<&mut Vec<Value>, Box<dyn Error>> {
    value
        .pointer_mut("/active/mise_runtime")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| io::Error::other("active runtime approval missing").into())
}

fn remove_runtime_target(value: &mut Value) -> Result<(), Box<dyn Error>> {
    runtime_entries_mut(value)?
        .pop()
        .ok_or_else(|| io::Error::other("runtime entry missing"))?;
    Ok(())
}

fn duplicate_runtime_target(value: &mut Value) -> Result<(), Box<dyn Error>> {
    let entries = runtime_entries_mut(value)?;
    let target = entries
        .first()
        .and_then(|entry| entry.get("target"))
        .cloned()
        .ok_or_else(|| io::Error::other("runtime target missing"))?;
    entries
        .get_mut(1)
        .ok_or_else(|| io::Error::other("second runtime entry missing"))?["target"] = target;
    Ok(())
}

fn unknown_runtime_target(value: &mut Value) -> Result<(), Box<dyn Error>> {
    runtime_entries_mut(value)?
        .first_mut()
        .ok_or_else(|| io::Error::other("runtime entry missing"))?["target"] =
        Value::String("wasm32-unknown-unknown".to_owned());
    Ok(())
}

fn inconsistent_runtime_version(value: &mut Value) -> Result<(), Box<dyn Error>> {
    runtime_entries_mut(value)?
        .get_mut(1)
        .ok_or_else(|| io::Error::other("second runtime entry missing"))?["version"] =
        Value::String("2026.10.18".to_owned());
    Ok(())
}

fn malformed_runtime_version(value: &mut Value) -> Result<(), Box<dyn Error>> {
    runtime_entries_mut(value)?
        .first_mut()
        .ok_or_else(|| io::Error::other("runtime entry missing"))?["version"] =
        Value::String("2026.010.19".to_owned());
    Ok(())
}

fn malformed_runtime_hash(value: &mut Value) -> Result<(), Box<dyn Error>> {
    runtime_entries_mut(value)?
        .first_mut()
        .ok_or_else(|| io::Error::other("runtime entry missing"))?["sha256"] =
        Value::String("g".repeat(64));
    Ok(())
}

fn zero_runtime_bytes(value: &mut Value) -> Result<(), Box<dyn Error>> {
    runtime_entries_mut(value)?
        .first_mut()
        .ok_or_else(|| io::Error::other("runtime entry missing"))?["bytes"] = Value::from(0);
    Ok(())
}

type RuntimeMutation = fn(&mut Value) -> Result<(), Box<dyn Error>>;

#[test]
fn invalid_runtime_tables_fail_closed() -> Result<(), Box<dyn Error>> {
    let mutations: [(RuntimeMutation, &str); 7] = [
        (remove_runtime_target, "missing_runtime_target"),
        (duplicate_runtime_target, "duplicate_runtime_target"),
        (unknown_runtime_target, "unknown_runtime_target"),
        (inconsistent_runtime_version, "inconsistent_runtime_version"),
        (malformed_runtime_version, "malformed_runtime_version"),
        (malformed_runtime_hash, "malformed_runtime_sha256"),
        (zero_runtime_bytes, "zero_runtime_bytes"),
    ];
    for (mutate, problem) in mutations {
        let mut value: Value = serde_json::from_str(BASE_APPROVALS)?;
        mutate(&mut value)?;
        let approvals = serde_json::to_string(&value)?;
        let temp = fixture(Some(&approvals), BASE_LOCK)?;
        let error = prepare_error(temp.path())?;
        assert!(error.contains(problem), "{problem}: {error}");
    }
    Ok(())
}
