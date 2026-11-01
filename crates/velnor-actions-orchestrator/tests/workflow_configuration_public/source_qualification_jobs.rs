// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::source_qualification::{WORKFLOW_PATH, generator_repo};
use super::support::rendered_job_block;

const GATHER_NAME_LINE: &str = "    name: Source qualification\n";
const BUNDLE_PHASE: &str = "bundle-selected-qualification-evidence";
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

fn artifact_names(block: &str, action: &str) -> Vec<String> {
    steps(block)
        .into_iter()
        .filter(|step| step_field(step, "uses").is_some_and(|uses| uses.starts_with(action)))
        .filter_map(|step| {
            step.lines()
                .find_map(|line| line.strip_prefix("          name: "))
        })
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
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
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
    assert_eq!(rendered, script.iter().cloned().collect::<BTreeSet<_>>());
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
    Ok(())
}

#[test]
fn source_qualification_keeps_triggers_permissions_and_concurrency() -> Result<(), Box<dyn Error>> {
    let workflow = rendered_workflow()?;
    assert!(workflow.contains(
        "\"on\":\n  pull_request: {}\n  workflow_dispatch: {}\npermissions:\n  contents: read\nconcurrency:\n  group: source-qualification-${{ github.ref }}\n  cancel-in-progress: false\njobs:\n"
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
