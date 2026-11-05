// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::state_producer_eval::{
    HEALTHY, eval_expr, legs_of, needs_value, producer_outputs, simulate_leg,
};
use super::state_slices::{WORKSPACE, binned_repo, plan_outputs};

const WS_SLICES: [&str; 6] = [
    "small",
    "rust_builddep",
    "rust_devdep",
    "rust_leaf",
    "rust_mid",
    "rust_other",
];

fn snapshots(
    jobs: &BTreeMap<String, Job>,
    repo: &Path,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let snapshots: BTreeMap<String, String> = plan_outputs(jobs, repo)?
        .into_iter()
        .filter_map(|(name, value)| {
            let slice = name
                .strip_prefix("state_slice_")?
                .strip_suffix("_snapshot")?;
            Some((slice.to_owned(), value))
        })
        .collect();
    let mut expected: BTreeSet<&str> = WS_SLICES.into_iter().collect();
    expected.insert("rust_nested");
    if snapshots
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != expected
    {
        return Err(format!("expected one snapshot per slice: {snapshots:?}").into());
    }
    Ok(snapshots)
}

fn moved(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> BTreeSet<String> {
    before
        .iter()
        .filter(|(slice, value)| after.get(*slice) != Some(*value))
        .map(|(slice, _)| slice.clone())
        .collect()
}

fn expected(slices: &[&str]) -> BTreeSet<String> {
    slices.iter().map(|slice| (*slice).to_owned()).collect()
}

#[test]
fn a_source_edit_reproduces_only_the_slices_whose_closure_holds_it() -> Result<(), Box<dyn Error>> {
    let repo = binned_repo()?;
    let root = repo.path();
    let jobs = finalized_jobs(&prepare(root)?)?;
    let base = snapshots(&jobs, root)?;
    let cases: [(&str, &[&str]); 6] = [
        ("leaf/src/lib.rs", &["rust_leaf", "rust_mid"]),
        ("mid/src/lib.rs", &["rust_mid"]),
        ("bina/src/lib.rs", &["small"]),
        ("devdep/src/lib.rs", &["rust_devdep", "rust_other"]),
        ("builddep/src/lib.rs", &["rust_builddep", "rust_other"]),
        (".cargo/config.toml", &WS_SLICES),
    ];
    for (file, wanted) in cases {
        let path = root.join(WORKSPACE).join(file);
        let original = fs::read_to_string(&path).ok();
        fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        fs::write(&path, "pub fn g() {}\n")?;
        let after = snapshots(&jobs, root)?;
        match original {
            Some(text) => fs::write(&path, text)?,
            None => fs::remove_file(&path)?,
        }
        assert_eq!(moved(&base, &after), expected(wanted), "editing {file}");
    }
    assert_eq!(snapshots(&jobs, root)?, base, "restoring every edit");
    Ok(())
}

#[test]
fn a_lane_change_of_one_job_reproduces_only_its_slice() -> Result<(), Box<dyn Error>> {
    let repo = binned_repo()?;
    let root = repo.path();
    let base = snapshots(&finalized_jobs(&prepare(root)?)?, root)?;
    let config = root.join(".velnor/config.toml");
    let original = fs::read_to_string(&config)?;
    for (package, wanted) in [("mid", "rust_mid"), ("bina", "small")] {
        fs::write(
            &config,
            format!("{original}\n[stacks.rust.crates.{package}]\nbuild_jobs = 1\n"),
        )?;
        let after = snapshots(&finalized_jobs(&prepare(root)?)?, root)?;
        assert_eq!(moved(&base, &after), expected(&[wanted]), "{package} lanes");
    }
    fs::write(&config, original)?;
    Ok(())
}

fn task_ids(job: &Job) -> BTreeSet<&str> {
    job.steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::TaskExecution { task_id, .. } => Some(task_id.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn each_producer_leg_runs_exactly_the_lanes_of_its_own_slice() -> Result<(), Box<dyn Error>> {
    let repo = binned_repo()?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let producer = jobs.get("state-producer-ws").ok_or("no ws producer")?;
    let slice_of = |job: &str| match job {
        "rust-bina" | "rust-binb" => "small".to_owned(),
        other => other.to_owned(),
    };
    for step in &producer.steps {
        let StepKind::TaskExecution { task_id, .. } = &step.kind else {
            continue;
        };
        let owner = jobs
            .iter()
            .find(|(id, job)| id.starts_with("rust-") && task_ids(job).contains(task_id.as_str()))
            .map(|(id, _)| slice_of(id))
            .ok_or_else(|| format!("{task_id} belongs to no crate job"))?;
        let gated = step
            .condition
            .as_deref()
            .and_then(velnor_actions_contract::workflow::state_seal::state_producer_lane_slice)
            .map(|slice| slice.to_string());
        assert_eq!(gated, Some(owner), "{task_id} is gated to another slice");
    }
    assert_eq!(
        legs_of(producer).len(),
        WS_SLICES.len(),
        "a leg per ws slice"
    );
    for leg in legs_of(producer) {
        let tasks: BTreeSet<&str> = jobs
            .iter()
            .filter(|(id, _)| id.starts_with("rust-") && slice_of(id) == leg)
            .flat_map(|(_, job)| task_ids(job))
            .collect();
        let owned = producer
            .steps
            .iter()
            .filter(|step| {
                matches!(&step.kind, StepKind::TaskExecution { task_id, .. } if tasks.contains(task_id.as_str()))
            })
            .count();
        assert_ne!(owned, 0, "{leg} owns no lane");
        assert_eq!(
            simulate_leg(producer, &leg, &HEALTHY)?.lanes,
            owned,
            "{leg} runs lanes of another slice"
        );
    }
    Ok(())
}

#[test]
fn each_seal_leg_downloads_only_the_artifact_its_own_producer_leg_uploaded()
-> Result<(), Box<dyn Error>> {
    let repo = binned_repo()?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let mut legs = Vec::new();
    let mut artifacts = BTreeMap::new();
    for (id, producer) in jobs
        .iter()
        .filter(|(id, _)| id.starts_with("state-producer-"))
    {
        legs.extend(legs_of(producer));
        artifacts.extend(producer_outputs(id, producer, &HEALTHY)?);
    }
    assert_eq!(legs.len(), WS_SLICES.len() + 1, "a leg per slice");
    let seal = jobs.get("state-seal").ok_or("no seal job")?;
    let download = seal
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::StateSealDownload))
        .ok_or("the seal job lost its download")?;
    let StepKind::Action { with, .. } = &download.kind else {
        return Err("the download left its action form".into());
    };
    assert!(
        !with.contains_key("name"),
        "the seal leg selects its payload by name"
    );
    let lookup = with
        .get("artifact-ids")
        .and_then(|value| value.strip_prefix("${{ "))
        .and_then(|value| value.strip_suffix(" }}"))
        .ok_or("the download names no artifact id")?;
    let value = |atom: &str| needs_value(atom, &artifacts);
    for leg in &legs {
        assert_eq!(
            eval_expr(lookup, leg, &value)?,
            format!("id-{leg}"),
            "leg {leg} downloads a payload its producer leg did not upload"
        );
    }
    Ok(())
}
