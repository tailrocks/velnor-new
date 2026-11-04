// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Output};

use velnor_actions_contract::ids::run_key_for_ci;
use velnor_actions_contract::{Job, Step, StepKind, StepRole};

use super::state_producer::{ROOTS, build, job, text_of};
use super::support::rendered_job_block;
use super::workflow_configuration_fixture::git;

const RUN_ID: &str = "7";
const ATTEMPT: &str = "1";
const OTHER_RUN_KEY: &str = "r8-a1";
const STUB: &str = "stub.rs";

fn job_env(ci: &str, job_id: &str) -> Result<String, Box<dyn Error>> {
    let block = rendered_job_block(ci, job_id)?;
    let (index, line) = block
        .lines()
        .enumerate()
        .find(|(_, line)| line.starts_with("    env:"))
        .ok_or("producer job has no env")?;
    let declaration = line.trim_start_matches("    env:").trim();
    let (text, skip) = match declaration.strip_prefix('*') {
        Some(anchor) => {
            let definition = format!("    env: &{anchor}\n");
            let start = ci.find(&definition).ok_or("env anchor is missing")? + definition.len();
            (&ci[start..], 0)
        }
        None => (block, index + 1),
    };
    Ok(text
        .lines()
        .skip(skip)
        .take_while(|line| line.starts_with("      "))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn lane_matrix_keys(producer: &Job) -> Vec<(&str, &str)> {
    let mut keys: Vec<(&str, &str)> = producer
        .steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::TaskExecution {
                task_id,
                matrix_key,
                ..
            } => Some((task_id.as_str(), matrix_key.as_str())),
            _ => None,
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

fn write_reports(
    runner_temp: &Path,
    run_key: &str,
    task_id: &str,
    matrix_key: &str,
) -> Result<(), Box<dyn Error>> {
    let dir = runner_temp.join("velnor").join(run_key).join(matrix_key);
    fs::create_dir_all(dir.join("tasks"))?;
    for file in [
        dir.join("matrix-report.json"),
        dir.join("tasks").join(format!("{matrix_key}.json")),
    ] {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(format!(
                    "internal: report_exists for {task_id} at {} \
                     (v0.1.72 task_report/observation.rs:306-315, exclusive_write.rs:31,94)",
                    file.display()
                )
                .into());
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn restore_step(producer: &Job) -> Result<&Step, Box<dyn Error>> {
    producer
        .steps
        .iter()
        .find(|step| step.role == Some(StepRole::StateSkeletonRestore))
        .ok_or_else(|| "producer lost its restore step".into())
}

fn run_restore(
    step: &Step,
    workspace: &Path,
    runner_temp: &Path,
    attempt: Option<&str>,
) -> Result<Output, Box<dyn Error>> {
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err("restore step left its shell form".into());
    };
    let (program, args) = run.split_first().ok_or("restore step has no command")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(workspace)
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_WORKSPACE", workspace)
        .env("GITHUB_ENV", runner_temp.join("github_env"))
        .env("MBX_CACHE_DIR", runner_temp.join("velnor/mbx"))
        .env("GITHUB_RUN_ID", RUN_ID)
        .env_remove("VELNOR_RUN_KEY_ATTEMPT");
    if let Some(attempt) = attempt {
        command.env("VELNOR_RUN_KEY_ATTEMPT", attempt);
    }
    Ok(command.output()?)
}

fn seed(workspace: &Path, runner_temp: &Path, run_key: &str) -> Result<(), Box<dyn Error>> {
    git(&["init", "-b", "testmain"], workspace)?;
    fs::write(workspace.join(STUB), "fn main() {}\n")?;
    git(&["add", STUB], workspace)?;
    git(
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-m",
            "seed",
        ],
        workspace,
    )?;
    fs::write(workspace.join(STUB), "")?;
    let skeleton = runner_temp.join("velnor/state-skeleton");
    fs::create_dir_all(&skeleton)?;
    fs::write(skeleton.join("stubbed"), format!("{STUB}\n"))?;
    fs::create_dir_all(workspace.join("target"))?;
    fs::create_dir_all(skeleton.join("task-manifests"))?;
    fs::write(
        skeleton.join("metadata.json"),
        format!(
            "{{\"target_directory\":\"{}\",\"packages\":[]}}",
            workspace.join("target").display()
        ),
    )?;
    let run_dir = runner_temp.join("velnor").join(run_key);
    fs::create_dir_all(&run_dir)?;
    fs::write(run_dir.join("plan.json"), "{}")?;
    fs::write(run_dir.join("final-report.json"), "{}")?;
    fs::create_dir_all(runner_temp.join("velnor/mbx/actions"))?;
    fs::write(runner_temp.join("velnor/mbx/actions/keep"), "")?;
    Ok(())
}

fn assert_kept(runner_temp: &Path, run_key: &str) {
    for kept in [
        format!("velnor/{run_key}/plan.json"),
        format!("velnor/{run_key}/final-report.json"),
        "velnor/mbx/actions/keep".to_owned(),
    ] {
        assert!(runner_temp.join(&kept).is_file(), "{kept} was removed");
    }
}

#[test]
fn the_second_producer_phase_can_write_the_task_reports_the_first_phase_wrote()
-> Result<(), Box<dyn Error>> {
    let built = build()?;
    let run_key = run_key_for_ci(RUN_ID.parse()?, ATTEMPT.parse()?);
    for (producer_id, _) in ROOTS {
        let producer = job(&built.jobs, producer_id)?;
        let lanes = lane_matrix_keys(producer);
        assert!(!lanes.is_empty(), "{producer_id} has no lane records");
        assert!(
            job_env(text_of(&built, producer_id), producer_id)?.contains("VELNOR_RUN_KEY_ATTEMPT"),
            "{producer_id} does not carry the run attempt the restore step needs"
        );
        let workspace = tempfile::tempdir()?;
        let temp = tempfile::tempdir()?;
        seed(workspace.path(), temp.path(), &run_key)?;
        for (task_id, matrix_key) in &lanes {
            write_reports(temp.path(), &run_key, task_id, matrix_key)?;
        }
        let other = temp
            .path()
            .join("velnor")
            .join(OTHER_RUN_KEY)
            .join("m-0000000000000000");
        fs::create_dir_all(&other)?;
        fs::write(other.join("matrix-report.json"), "{}")?;

        let restore = run_restore(
            restore_step(producer)?,
            workspace.path(),
            temp.path(),
            Some(ATTEMPT),
        )?;
        assert!(
            restore.status.success(),
            "{producer_id}: restore failed: {}",
            String::from_utf8_lossy(&restore.stderr)
        );
        for (task_id, matrix_key) in &lanes {
            write_reports(temp.path(), &run_key, task_id, matrix_key)?;
        }
        assert_kept(temp.path(), &run_key);
        assert!(
            other.join("matrix-report.json").is_file(),
            "{producer_id}: another run's reports were removed"
        );
    }
    Ok(())
}

#[test]
fn a_restore_without_the_run_attempt_leaves_the_task_reports_in_place() -> Result<(), Box<dyn Error>>
{
    let built = build()?;
    let run_key = run_key_for_ci(RUN_ID.parse()?, ATTEMPT.parse()?);
    let (producer_id, _) = ROOTS[0];
    let producer = job(&built.jobs, producer_id)?;
    let lanes = lane_matrix_keys(producer);
    let workspace = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    seed(workspace.path(), temp.path(), &run_key)?;
    for (task_id, matrix_key) in &lanes {
        write_reports(temp.path(), &run_key, task_id, matrix_key)?;
    }
    let restore = run_restore(restore_step(producer)?, workspace.path(), temp.path(), None)?;
    assert!(!restore.status.success(), "restore ran without an attempt");
    for (_, matrix_key) in &lanes {
        let report = temp
            .path()
            .join("velnor")
            .join(&run_key)
            .join(matrix_key)
            .join("matrix-report.json");
        assert!(report.is_file(), "{} was removed", report.display());
    }
    Ok(())
}
