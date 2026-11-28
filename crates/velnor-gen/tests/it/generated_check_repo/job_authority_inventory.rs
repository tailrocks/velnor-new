// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use serde_json::Value;
use velnor_gen::{Drift, drift, expected_with_notes, job_authority_identity};
use velnor_model::{
    JOB_AUTHORITY_CONFIG_PATH, JOB_AUTHORITY_INVENTORY_PATH, Platform, RepoPath, RepoView,
};
use velnor_rust::LOCK_PATH;

use super::job_authority_identity::{digest, running};
use crate::facts::TestResult;
use crate::golden::update_requested;
use crate::lock::COMMIT_FILE;
use crate::lock_archive::archive_lock_at;
use crate::private_sources::SECTION;
use crate::release_fixture::release_view;
use crate::yaml_tree::document;

pub(super) const CI: &str = ".github/workflows/ci.yml";
pub(super) const GATED: &str = ".github/workflows/gated.yml";
pub(super) const HELPER: &str = "0.1.111";

const GATED_JOBS: &str = r#"
[[workflow]]
file = "gated"
name = "Gated"

[workflow.on.push]
branches = ["main"]
{extra}
[[workflow.job]]
id = "plain"
permissions = { contents = "read" }

[[workflow.job.step]]
name = "Plain"
run = "echo ok"

[[workflow.job]]
id = "writer"
permissions = { contents = "write" }

[[workflow.job.step]]
name = "Write"
run = "echo ok"

[[workflow.job]]
id = "gated"
environment = "preview"
permissions = { contents = "read" }

[[workflow.job.step]]
name = "Gate"
run = "echo ok"
"#;

pub(super) fn pinned() -> &'static str {
    COMMIT_FILE.trim()
}

pub(super) fn enabled(
    rust_ci: &str,
    workflows: Option<&str>,
    listed: &[&str],
) -> Result<RepoView, Box<dyn Error>> {
    let mut view = release_view(&[], None)?;
    let mut put = |path: &str, text: &str| -> TestResult {
        let path = RepoPath::new(path)?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, text.as_bytes().to_vec());
        Ok(())
    };
    put(".velnor/rust-ci.toml", rust_ci)?;
    if let Some(workflows) = workflows {
        put(".velnor/workflows.toml", workflows)?;
    }
    let quoted: Vec<String> = listed.iter().map(|path| format!("\"{path}\"")).collect();
    put(
        JOB_AUTHORITY_CONFIG_PATH.as_str(),
        &format!("workflows = [{}]\n", quoted.join(", ")),
    )?;
    view.tracked_paths.insert(LOCK_PATH);
    view.files
        .insert(LOCK_PATH, archive_lock_at(HELPER)?.into_bytes());
    crate::facts::lock_pinned_tools(&mut view)?;
    Ok(view)
}

pub(super) fn gated(extra: &str) -> String {
    GATED_JOBS.replace("{extra}", extra)
}

pub(super) type Generated = (BTreeMap<RepoPath, String>, Vec<String>);

pub(super) fn generated(view: &RepoView) -> Result<Generated, Box<dyn Error>> {
    Ok(expected_with_notes(view)?)
}

fn inventory(files: &BTreeMap<RepoPath, String>) -> Result<Value, Box<dyn Error>> {
    let text = files
        .get(&JOB_AUTHORITY_INVENTORY_PATH)
        .ok_or("no inventory was written")?;
    Ok(serde_json::from_str(text)?)
}

fn job_keys<'a>(inventory: &'a Value, workflow: &str) -> Vec<&'a str> {
    inventory["workflows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["path"] == workflow)
        .flat_map(|entry| entry["jobs"].as_array().into_iter().flatten())
        .filter_map(|job| job["job_key"].as_str())
        .collect()
}

#[test]
fn inventory_repository_mode_has_literal_branch_filters() -> TestResult {
    let view = enabled("full_run = \"off\"\n", None, &[CI])?;
    let (files, _) = generated(&view)?;
    let inventory = inventory(&files)?;
    let mut seen = BTreeSet::new();
    for workflow in inventory["workflows"].as_array().ok_or("workflows")? {
        for job in workflow["jobs"].as_array().ok_or("jobs")? {
            for context in job["contexts"].as_array().ok_or("contexts")? {
                let event = context["event"].as_str().ok_or("event")?;
                if event == "pull_request" {
                    continue;
                }
                assert_eq!(
                    context["branch_filters"],
                    serde_json::json!(["main"]),
                    "{event} of {}",
                    job["job_key"]
                );
                seen.insert(event.to_owned());
            }
        }
    }
    assert_eq!(seen, ["merge_group", "push"].map(str::to_owned).into());
    Ok(())
}

#[test]
fn check_tracks_the_committed_inventory_byte_for_byte() -> TestResult {
    let view = enabled("full_run = \"off\"\n", None, &[CI])?;
    let (files, _) = generated(&view)?;
    let mut on_disk: BTreeMap<RepoPath, Vec<u8>> = BTreeMap::new();
    let text = &files[&JOB_AUTHORITY_INVENTORY_PATH];
    on_disk.insert(JOB_AUTHORITY_INVENTORY_PATH, text.clone().into_bytes());
    let none = BTreeSet::new();
    let drifts = |on_disk: &BTreeMap<RepoPath, Vec<u8>>| -> Vec<String> {
        drift(&files, on_disk, &none)
            .into_iter()
            .filter(|drift| drift.to_string().contains("job-authority"))
            .map(|drift| drift.to_string())
            .collect()
    };
    assert_eq!(drifts(&on_disk), Vec::<String>::new());
    on_disk.insert(
        JOB_AUTHORITY_INVENTORY_PATH,
        text.replacen("\"cache\"", "\"kache\"", 1).into_bytes(),
    );
    let changed = drift(&files, &on_disk, &none);
    assert!(
        changed.contains(&Drift::Changed(JOB_AUTHORITY_INVENTORY_PATH)),
        "{changed:?}"
    );
    let mut files = files;
    files.remove(&JOB_AUTHORITY_INVENTORY_PATH);
    let stale = drift(&files, &on_disk, &none);
    assert!(
        stale.contains(&Drift::Stale(JOB_AUTHORITY_INVENTORY_PATH)),
        "{stale:?}"
    );
    Ok(())
}

#[test]
fn a_job_that_writes_or_names_an_environment_is_left_out_and_named() -> TestResult {
    let view = enabled("full_run = \"off\"\n", Some(&gated("")), &[CI, GATED])?;
    let (files, notes) = generated(&view)?;
    let inventory = inventory(&files)?;
    assert_eq!(job_keys(&inventory, GATED), ["plain"]);
    assert_ne!(job_keys(&inventory, CI).len(), 0);
    for job in ["writer", "gated"] {
        assert!(
            notes.iter().any(
                |note| note.contains(GATED) && note.contains(&format!("job {job} is left out"))
            ),
            "{job}: {notes:?}"
        );
    }
    Ok(())
}

#[test]
fn a_job_that_uses_a_secret_is_left_out_and_named() -> TestResult {
    let rust_ci = format!("full_run = \"off\"\n\n{SECTION}");
    let view = enabled(&rust_ci, None, &[CI])?;
    let (files, notes) = generated(&view)?;
    let workflow = document(files.get(&RepoPath::new(CI)?).ok_or("no ci.yml")?)?;
    let jobs = workflow["jobs"].as_object().ok_or("jobs")?;
    let secret: Vec<&str> = jobs
        .iter()
        .filter(|(_, job)| job.to_string().contains("secrets."))
        .map(|(key, _)| key.as_str())
        .collect();
    assert!(!secret.is_empty(), "no generated job uses a secret");
    let inventory = inventory(&files)?;
    let kept = job_keys(&inventory, CI);
    assert_ne!(kept.len(), 0);
    for key in secret {
        assert!(!kept.contains(&key), "{key} is in the inventory");
        assert!(
            notes
                .iter()
                .any(|note| note.contains(&format!("job {key} is left out"))),
            "{key}: {notes:?}"
        );
    }
    Ok(())
}

#[test]
fn schedule_and_workflow_dispatch_are_named_in_a_note_and_not_in_the_inventory() -> TestResult {
    let extra = "\n[workflow.on]\nschedule = [\"23 4 * * 1\"]\n\n[workflow.on.workflow_dispatch]\n";
    let view = enabled("full_run = \"off\"\n", Some(&gated(extra)), &[GATED])?;
    let (files, notes) = generated(&view)?;
    assert!(
        notes.iter().any(|note| note.contains(GATED)
            && note.contains("schedule and workflow_dispatch")
            && note.contains("does not cover")),
        "{notes:?}"
    );
    let inventory = inventory(&files)?;
    let events: BTreeSet<&str> = inventory["workflows"][0]["jobs"][0]["contexts"]
        .as_array()
        .ok_or("contexts")?
        .iter()
        .filter_map(|context| context["event"].as_str())
        .collect();
    assert_eq!(events, BTreeSet::from(["push"]));
    let view = enabled("full_run = \"off\"\n", Some(&gated("")), &[GATED])?;
    let (_, notes) = generated(&view)?;
    assert!(
        notes.iter().all(|note| !note.contains("does not cover")),
        "{notes:?}"
    );
    Ok(())
}

#[test]
fn the_fixture_the_host_connects_with_is_what_velnor_gen_writes() -> TestResult {
    let view = enabled("full_run = \"off\"\n", None, &[CI])?;
    let (files, _) = generated(&view)?;
    let digest = digest(&view, "linux-arm64")?;
    let running = running(Some(Platform::LinuxArm64), &digest);
    let identity = job_authority_identity(&view, running)?.ok_or("no identity")?;
    let wanted = [
        (
            "inventory.json",
            files[&JOB_AUTHORITY_INVENTORY_PATH].clone(),
        ),
        ("identity.txt", identity.lines()),
    ];
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../velnor-runner-cli/tests/data/job-authority");
    let update = update_requested(std::env::var_os("UPDATE_GOLDENS").as_deref());
    for (name, text) in wanted {
        let path = directory.join(name);
        if update {
            std::fs::create_dir_all(&directory)?;
            std::fs::write(&path, &text)?;
        }
        assert_eq!(
            std::fs::read_to_string(&path)?,
            text,
            "{name} is stale; run with UPDATE_GOLDENS=1"
        );
    }
    Ok(())
}
