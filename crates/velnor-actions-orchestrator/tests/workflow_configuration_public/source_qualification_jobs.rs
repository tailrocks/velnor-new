// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::source_qualification::{WORKFLOW_PATH, generator_repo};
use super::support::rendered_job_block;

const GATHER_NAME_LINE: &str = "    name: Source qualification\n";
const BUNDLE_PHASE: &str = "bundle-selected-qualification-evidence";
const ALWAYS_PHASES: &[&str] = &[
    "prepare-external-evidence-directories",
    "capture-all-five-external-goldens",
    "run-full-golden-shell-harness-against-external-oracle",
    "preserve-ordinary-output-binding-json-proof-files",
    "run-full-golden-shell-harness-with-retained-workspaces",
    "preserve-keep-output-binding-json-proof-files",
    "repeat-check-against-external-oracle",
    "generate-twice-and-retain-root-to-preview-identities",
    "test-generated-rust-verifier-transport-and-digest-gate",
    "check-source-output-against-checked-in-repository-goldens",
];
const LAST_PRELUDE_PHASE: &str = "capture-all-five-external-goldens";

fn rendered_workflow() -> Result<String, Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(tree
        .get(WORKFLOW_PATH)
        .ok_or("source qualification workflow was not rendered")?
        .to_string())
}

fn script_phases() -> Result<Vec<String>, Box<dyn Error>> {
    let script = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/generator-release/source-qualification.sh"),
    )?;
    let mut phases = Vec::new();
    for line in script.lines() {
        let Some(labels) = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(')'))
        else {
            continue;
        };
        if labels.is_empty()
            || !labels
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-' || c == '|')
        {
            continue;
        }
        phases.extend(labels.split('|').map(str::to_owned));
    }
    Ok(phases)
}

fn job_ids(workflow: &str) -> Vec<String> {
    let jobs = workflow.split("\njobs:\n").nth(1).unwrap_or_default();
    jobs.lines()
        .filter_map(|line| {
            let id = line.strip_prefix("  ")?.strip_suffix(':')?;
            (!id.is_empty() && !id.starts_with(' ')).then(|| id.to_owned())
        })
        .collect()
}

fn job_phases(block: &str) -> Vec<String> {
    block
        .lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("run: bash scripts/generator-release/source-qualification.sh ")
                .map(str::to_owned)
        })
        .collect()
}

fn gather_id(workflow: &str) -> Result<String, Box<dyn Error>> {
    let gathers: Vec<String> = job_ids(workflow)
        .into_iter()
        .filter(|id| {
            rendered_job_block(workflow, id).is_ok_and(|block| block.contains(GATHER_NAME_LINE))
        })
        .collect();
    match gathers.as_slice() {
        [only] => Ok(only.clone()),
        other => {
            Err(format!("expected one job named Source qualification, found {other:?}").into())
        }
    }
}

fn steps(block: &str) -> Vec<&str> {
    let block = block
        .split_once("\n    steps:")
        .map_or("", |(_, steps)| steps);
    let starts: Vec<usize> = block
        .match_indices("\n      - ")
        .map(|(index, _)| index + 1)
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(position, start)| {
            let end = starts.get(position + 1).copied().unwrap_or(block.len());
            &block[*start..end]
        })
        .collect()
}

fn step_field<'a>(step: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("{key}: ");
    step.lines()
        .find_map(|line| line.trim().trim_start_matches("- ").strip_prefix(&marker))
}

fn with_field<'a>(step: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("          {key}: ");
    step.lines().find_map(|line| line.strip_prefix(&marker))
}

fn action_steps<'a>(block: &'a str, action: &str) -> Vec<&'a str> {
    steps(block)
        .into_iter()
        .filter(|step| step_field(step, "uses").is_some_and(|uses| uses.starts_with(action)))
        .collect()
}

fn artifact_names(block: &str, action: &str) -> Vec<String> {
    action_steps(block, action)
        .into_iter()
        .filter_map(|step| with_field(step, "name"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn source_qualification_phases_are_all_scheduled_across_jobs() -> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow()?;
    let script = script_phases()?;
    let gather = gather_id(&workflow)?;
    let prelude_end = script
        .iter()
        .position(|phase| phase == LAST_PRELUDE_PHASE)
        .ok_or("script has no capture phase")?;
    let prelude = &script[..=prelude_end];
    let mut rendered = BTreeSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for id in job_ids(&workflow) {
        let phases = job_phases(rendered_job_block(&workflow, &id)?);
        if id == gather {
            assert_eq!(phases, [BUNDLE_PHASE], "gather job {id} runs other phases");
            continue;
        }
        assert!(
            !phases.iter().any(|phase| phase == BUNDLE_PHASE),
            "producer job {id} runs the bundle phase"
        );
        for phase in prelude {
            assert_eq!(
                phases
                    .iter()
                    .filter(|candidate| *candidate == phase)
                    .count(),
                1,
                "job {id} must run prelude phase {phase} exactly once: {phases:?}"
            );
        }
        for phase in phases {
            *counts.entry(phase.clone()).or_default() += 1;
            rendered.insert(phase);
        }
    }
    rendered.insert(BUNDLE_PHASE.to_owned());
    let scripted: BTreeSet<String> = script.iter().cloned().collect();
    let unscheduled: Vec<&String> = scripted.difference(&rendered).collect();
    let unknown: Vec<&String> = rendered.difference(&scripted).collect();
    assert!(
        unscheduled.is_empty() && unknown.is_empty(),
        "phases in the script but in no job: {unscheduled:?}; phases in a job but not in the script: {unknown:?}"
    );
    for phase in &script[prelude_end + 1..] {
        if phase != BUNDLE_PHASE {
            assert_eq!(
                counts.get(phase),
                Some(&1),
                "phase {phase} must run in exactly one job"
            );
        }
    }
    Ok(())
}

fn assert_producer_gate(gather_block: &str, producers: &[String]) -> Result<(), Box<dyn Error>> {
    let needs: Vec<&str> = gather_block
        .lines()
        .skip_while(|line| *line != "    needs:")
        .skip(1)
        .map_while(|line| line.strip_prefix("      - "))
        .collect();
    assert_eq!(
        needs,
        producers.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert!(gather_block.contains("\n    if: always()\n"));
    let gather_steps = steps(gather_block);
    let guard = *gather_steps.last().ok_or("gather job has no steps")?;
    let expected_condition = producers
        .iter()
        .map(|id| format!("needs.{id}.result != 'success'"))
        .collect::<Vec<_>>()
        .join(" || ");
    assert_eq!(step_field(guard, "if"), Some(expected_condition.as_str()));
    assert!(
        guard.contains("exit 1"),
        "last gather step does not fail: {guard}"
    );
    let position =
        |predicate: &dyn Fn(&str) -> bool| gather_steps.iter().position(|s| predicate(s));
    let checkout = position(&|step| {
        step_field(step, "uses").is_some_and(|uses| uses.starts_with("actions/checkout@"))
    })
    .ok_or("gather job has no checkout")?;
    let first_download = position(&|step| {
        step_field(step, "uses").is_some_and(|uses| uses.starts_with("actions/download-artifact@"))
    })
    .ok_or("gather job has no download")?;
    let bundle =
        position(&|step| step_field(step, "run").is_some_and(|run| run.ends_with(BUNDLE_PHASE)))
            .ok_or("gather job has no bundle step")?;
    assert!(
        checkout < first_download && checkout < bundle,
        "checkout must precede download and bundle: {gather_steps:?}"
    );
    assert!(
        step_field(gather_steps[checkout], "if").is_none(),
        "checkout is conditional: {}",
        gather_steps[checkout]
    );
    assert!(
        bundle < gather_steps.len() - 1,
        "guard must follow the bundle: {gather_steps:?}"
    );
    Ok(())
}

fn assert_download_paths(
    workflow: &str,
    gather_block: &str,
    producers: &[String],
) -> Result<(), Box<dyn Error>> {
    let downloads = action_steps(gather_block, "actions/download-artifact@");
    let mut paths = BTreeSet::new();
    for id in producers {
        let upload_name = artifact_names(
            rendered_job_block(workflow, id)?,
            "actions/upload-artifact@",
        )
        .pop()
        .ok_or("producer upload missing")?;
        let paired: Vec<&&str> = downloads
            .iter()
            .filter(|step| with_field(step, "name") == Some(upload_name.as_str()))
            .collect();
        let [download] = paired.as_slice() else {
            return Err(format!("producer {id} needs exactly one download: {paired:?}").into());
        };
        let path = with_field(download, "path").ok_or("download has no path")?;
        assert!(
            path.ends_with(&format!("/source-qualification-jobs/{id}")),
            "download of {upload_name} lands in {path}"
        );
        assert!(paths.insert(path), "download path {path} is shared");
    }
    Ok(())
}

#[test]
fn source_qualification_gather_job_keeps_the_check_name_and_needs_every_producer()
-> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow()?;
    let gather = gather_id(&workflow)?;
    assert_eq!(workflow.matches(GATHER_NAME_LINE).count(), 1);
    let producers: Vec<String> = job_ids(&workflow)
        .into_iter()
        .filter(|id| *id != gather)
        .collect();
    assert!(
        producers.len() >= 2,
        "expected parallel producers: {producers:?}"
    );

    let gather_block = rendered_job_block(&workflow, &gather)?;
    assert_producer_gate(gather_block, &producers)?;

    let mut uploaded = BTreeSet::new();
    for id in &producers {
        let block = rendered_job_block(&workflow, id)?;
        assert!(
            !block.contains("\n    needs:"),
            "producer {id} must not wait"
        );
        let uploads = artifact_names(block, "actions/upload-artifact@");
        let [name] = uploads.as_slice() else {
            return Err(format!("producer {id} must upload one artifact: {uploads:?}").into());
        };
        assert!(
            name.contains("${{ github.run_id }}") && name.contains("${{ github.run_attempt }}"),
            "artifact {name} is not scoped to the run attempt"
        );
        let upload = steps(block)
            .into_iter()
            .find(|step| step.contains("actions/upload-artifact@"))
            .ok_or("upload step missing")?;
        assert!(
            upload.contains("if: always()"),
            "producer {id} upload is conditional"
        );
        assert!(upload.contains("if-no-files-found: error"));
        assert!(
            uploaded.insert(name.clone()),
            "artifact {name} is not distinct"
        );
    }
    let downloaded: BTreeSet<String> = artifact_names(gather_block, "actions/download-artifact@")
        .into_iter()
        .collect();
    assert_eq!(downloaded, uploaded);

    assert_download_paths(&workflow, gather_block, &producers)
}

#[test]
fn source_qualification_harness_and_check_phases_keep_running_after_a_failure()
-> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow()?;
    let gather = gather_id(&workflow)?;
    for id in job_ids(&workflow).into_iter().filter(|id| *id != gather) {
        let block = rendered_job_block(&workflow, &id)?;
        let mut always_phases = BTreeSet::new();
        for step in steps(block) {
            let Some(phase) = step_field(step, "run").and_then(|run| {
                run.strip_prefix("bash scripts/generator-release/source-qualification.sh ")
            }) else {
                continue;
            };
            let always = step_field(step, "if") == Some("always()");
            if ALWAYS_PHASES.contains(&phase) {
                assert!(always, "job {id} phase {phase} lost if: always()");
                always_phases.insert(phase);
            } else {
                assert!(!always, "job {id} phase {phase} must stop after a failure");
            }
        }
        let job_phases = job_phases(block);
        assert!(
            job_phases.len() > always_phases.len(),
            "job {id} runs no gating phase"
        );
        assert!(
            always_phases.contains("capture-all-five-external-goldens"),
            "job {id} lost its capture phase"
        );
    }
    Ok(())
}

#[test]
fn source_qualification_keeps_triggers_permissions_and_concurrency() -> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow()?;
    assert!(workflow.contains(
        "\"on\":\n  pull_request: {}\n  workflow_dispatch: {}\npermissions:\n  contents: read\ncache-mode: read\nconcurrency:\n  group: source-qualification-${{ github.ref }}\n  cancel-in-progress: false\njobs:\n"
    ));
    let ids = job_ids(&workflow);
    assert_eq!(workflow.matches("concurrency:").count(), 1);
    for id in &ids {
        let block = rendered_job_block(&workflow, id)?;
        assert!(
            block.contains("\n    timeout-minutes: "),
            "job {id} has no timeout"
        );
        assert!(
            block.contains("\n    permissions:\n      contents: read\n"),
            "job {id} permissions are not read-only"
        );
        assert!(
            !block.contains("write"),
            "job {id} grants a write permission"
        );
    }
    Ok(())
}
