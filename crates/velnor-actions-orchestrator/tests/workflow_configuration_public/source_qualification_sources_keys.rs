// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::source_qualification_prefetch_harness::{Scenario, run_prefetch};
use super::source_qualification_sources_support::{
    RUNNER_ROOT, Rendered, producer_ids, render, sources_repo, steps_of, text,
};
use super::workflow_configuration_fixture::write_root_cargo_lock;

fn cache_keys_outputs(
    workflow: &Value,
    job: &str,
    workspace: &Path,
    runner_temp: &Path,
    event: (&str, &str, &str),
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let step = steps_of(workflow, job)?
        .iter()
        .find(|step| step["id"] == "cache-keys")
        .ok_or_else(|| format!("job {job} has no cache keys step"))?;
    let script = text(&step["run"], "cache keys script")?;
    let output = runner_temp.join(format!("{job}-output"));
    fs::write(&output, "")?;
    let mut command = Command::new("sh");
    command
        .args(["-c", script])
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_WORKSPACE", workspace)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_EVENT_NAME", event.0)
        .env("GITHUB_REF", event.1)
        .env("GITHUB_REF_PROTECTED", event.2);
    for (name, value) in step["env"].as_object().ok_or("cache keys has no env")? {
        let value = text(value, name)?;
        let resolved = if name == "VELNOR_DEFAULT_REF" {
            "refs/heads/testmain"
        } else {
            value
        };
        command.env(name, resolved);
    }
    let run = command.output()?;
    if !run.status.success() {
        return Err(String::from_utf8_lossy(&run.stderr).into_owned().into());
    }
    Ok(parse_outputs(&fs::read_to_string(&output)?))
}

fn parse_outputs(text: &str) -> BTreeMap<String, String> {
    let mut outputs = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some((name, delimiter)) = line.split_once("<<") {
            let body: Vec<&str> = lines.by_ref().take_while(|l| *l != delimiter).collect();
            outputs.insert(name.to_owned(), body.join("\n"));
        } else if let Some((name, value)) = line.split_once('=') {
            outputs.insert(name.to_owned(), value.to_owned());
        }
    }
    outputs
}

const PLAN_EVENT: (&str, &str, &str) = ("push", "refs/heads/testmain", "true");
const PULL_REQUEST_EVENT: (&str, &str, &str) = ("pull_request", "refs/pull/7/merge", "false");

struct Keys {
    plan: BTreeMap<String, String>,
    harness: BTreeMap<String, String>,
}

fn keys(rendered: &Rendered, repo: &Path, runner_temp: &Path) -> Result<Keys, Box<dyn Error>> {
    let job = producer_ids(&rendered.sq)
        .into_iter()
        .next()
        .ok_or("no producer job")?;
    Ok(Keys {
        plan: cache_keys_outputs(&rendered.ci, "plan", repo, runner_temp, PLAN_EVENT)?,
        harness: cache_keys_outputs(&rendered.sq, &job, repo, runner_temp, PULL_REQUEST_EVENT)?,
    })
}

fn output<'a>(map: &'a BTreeMap<String, String>, name: &str) -> Result<&'a str, Box<dyn Error>> {
    map.get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("missing output {name}: {map:?}").into())
}

#[test]
fn the_harness_computes_the_same_sources_key_as_the_ci_plan() -> Result<(), Box<dyn Error>> {
    let repo = sources_repo()?;
    let rendered = render(repo.path())?;
    let temp = tempfile::tempdir()?;
    let before = keys(&rendered, repo.path(), temp.path())?;
    let plan_key = output(&before.plan, "sources_key")?;
    assert_ne!(plan_key, "");
    assert_eq!(plan_key, output(&before.harness, "sources_key")?);
    let restore_keys = output(&before.harness, "sources_restore_keys")?;
    assert_eq!(restore_keys, output(&before.plan, "sources_restore_keys")?);
    assert_ne!(restore_keys, "");
    assert!(
        plan_key.starts_with(restore_keys),
        "{plan_key} does not extend {restore_keys}"
    );
    assert_eq!(output(&before.plan, "sources_save")?, "true");
    assert_eq!(output(&before.harness, "sources_save")?, "false");

    write_root_cargo_lock(repo.path(), &["demo", "another"])?;
    let after = keys(&rendered, repo.path(), temp.path())?;
    let moved = output(&after.harness, "sources_key")?;
    assert_ne!(moved, plan_key);
    assert_eq!(moved, output(&after.plan, "sources_key")?);
    assert_eq!(
        output(&after.harness, "sources_restore_keys")?,
        restore_keys
    );
    Ok(())
}

fn lock_files(repo: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let listing = Command::new("git")
        .args(["ls-files", "--", "Cargo.lock", "**/Cargo.lock"])
        .current_dir(repo)
        .output()?;
    Ok(String::from_utf8(listing.stdout)?
        .lines()
        .map(str::to_owned)
        .collect())
}

#[test]
fn the_prefetched_manifests_are_exactly_the_locks_the_sources_key_covers()
-> Result<(), Box<dyn Error>> {
    let repo = sources_repo()?;
    let rendered = render(repo.path())?;
    let temp = tempfile::tempdir()?;
    let baseline = keys(&rendered, repo.path(), temp.path())?;
    let baseline_key = output(&baseline.harness, "sources_key")?.to_owned();

    let mut covered = BTreeSet::new();
    for lock in lock_files(repo.path())? {
        let path = repo.path().join(&lock);
        let original = fs::read_to_string(&path)?;
        fs::write(&path, format!("{original}# drift\n"))?;
        let drifted = keys(&rendered, repo.path(), temp.path());
        fs::write(&path, original)?;
        if output(&drifted?.harness, "sources_key")? != baseline_key {
            covered.insert(lock);
        }
    }
    assert_eq!(
        covered,
        BTreeSet::from(["Cargo.lock".to_owned(), format!("{RUNNER_ROOT}/Cargo.lock")])
    );

    let outcome = run_prefetch(
        repo.path(),
        &Scenario {
            cache_hit: "true",
            matched_key: "velnor-v2-sources-trusted-k",
            offline_ok: &["Cargo.toml", "crates/velnor-runner/Cargo.toml"],
            network_fails: &[],
        },
    )?;
    assert_eq!(outcome.status, Some(0));
    let hashed: BTreeSet<String> = outcome.lock_names.iter().cloned().collect();
    assert_eq!(hashed, covered, "hashed locks differ from keyed locks");
    let fetched: BTreeSet<String> = outcome
        .fetched_manifests()
        .iter()
        .map(|manifest| manifest.replace("Cargo.toml", "Cargo.lock"))
        .collect();
    assert_eq!(
        fetched, covered,
        "fetched manifests differ from keyed locks"
    );
    Ok(())
}
