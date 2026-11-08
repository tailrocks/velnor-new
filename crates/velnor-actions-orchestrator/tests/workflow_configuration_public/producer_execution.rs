// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::producer_execution_fixture::{Fixture, git_head};
use super::workflow_configuration_fixture::git;
use serde_json::json;
use std::{error::Error, fs};
use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::producer_execution_manifest::PRODUCER_EXECUTION_MANIFEST_PATH;
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn exercise_tasks(f: &Fixture) -> TestResult<(String, String, String, usize)> {
    let mut first = None;
    let mut count = 0;
    for (job, body) in f
        .jobs
        .iter()
        .filter(|(id, _)| id.starts_with("state-producer-"))
    {
        let other = f
            .jobs
            .keys()
            .find(|id| id.starts_with("state-producer-") && *id != job)
            .ok_or("second producer missing")?;
        for step in &body.steps {
            let StepKind::TaskExecution {
                task_id, matrix_id, ..
            } = &step.kind
            else {
                continue;
            };
            let record = f
                .manifest
                .tasks
                .values()
                .find(|record| record.task_id == *task_id && record.matrix_id == *matrix_id)
                .ok_or("task manifest entry missing")?;
            let digest = &record.execution_digest;
            let output = f.run(job, task_id, digest, &f.head, None)?;
            assert!(
                output.status.success(),
                "producer {job} cannot resolve {task_id}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            for (wrong_job, wrong_digest) in [
                (other.as_str(), digest.as_str()),
                ("state-producer-unrelated", digest.as_str()),
                (
                    job.as_str(),
                    "b3-0000000000000000000000000000000000000000000000000000000000000000",
                ),
            ] {
                assert!(
                    !f.run(wrong_job, task_id, wrong_digest, &f.head, None)?
                        .status
                        .success(),
                    "unauthorized producer execution succeeded"
                );
            }
            for exit in ["0", "1"] {
                let output = f.run(job, task_id, digest, &f.head, Some(exit))?;
                assert!(
                    output.status.success(),
                    "producer {job} cannot report {task_id}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            first.get_or_insert((job.clone(), task_id.clone(), digest.clone()));
            count += 1;
        }
    }
    let (job, task, digest) = first.ok_or("fixture has no producer tasks")?;
    Ok((job, task, digest, count))
}

fn reject_altered_plans(f: &Fixture, job: &str, task: &str, digest: &str) -> TestResult {
    let mut missing = f.plan.clone();
    missing["obligations"]
        .as_array_mut()
        .ok_or("obligations missing")?
        .retain(|entry| entry["task_id"] != task);
    f.write_plan(&missing)?;
    assert!(
        !f.run(job, task, digest, &f.head, None)?.status.success(),
        "missing obligation succeeded"
    );
    let mut mismatched = f.plan.clone();
    let entry = mismatched["matrix"]["include"]
        .as_array_mut()
        .ok_or("matrix missing")?
        .iter_mut()
        .find(|entry| entry["task_id"] == task)
        .ok_or("task entry missing")?;
    entry["task_digest"] =
        json!("b3-0000000000000000000000000000000000000000000000000000000000000000");
    f.write_plan(&mismatched)?;
    for exit in [None, Some("0")] {
        assert!(
            !f.run(job, task, digest, &f.head, exit)?.status.success(),
            "mismatched planned digest succeeded"
        );
    }
    let mut pull_request = f.plan.clone();
    pull_request["event"] = json!("pull_request");
    pull_request["trust"] = json!("pr");
    f.write_plan(&pull_request)?;
    for exit in [None, Some("0")] {
        let output = f.run(job, task, digest, &f.head, exit)?;
        assert!(!output.status.success(), "pull request producer succeeded");
    }
    f.write_plan(&f.plan)?;
    for exit in [None, Some("0")] {
        for (name, value) in [
            ("GITHUB_REF_PROTECTED", "false"),
            ("GITHUB_REF", "refs/heads/other"),
        ] {
            let output = f
                .command(job, task, digest, &f.head, exit)?
                .env(name, value)
                .output()?;
            assert!(
                !output.status.success(),
                "unauthorized push context succeeded"
            );
        }
    }
    Ok(())
}

fn reject_forged_membership(f: &Fixture, job: &str, task: &str, digest: &str) -> TestResult {
    f.write_plan(&f.plan)?;
    let other = f
        .jobs
        .keys()
        .find(|id| id.starts_with("state-producer-") && id.as_str() != job)
        .ok_or("other producer missing")?;
    let mut descriptor = f.descriptor()?;
    descriptor
        .producer_tasks
        .entry(other.clone())
        .or_default()
        .insert(task.to_owned(), digest.to_owned());
    f.write_descriptor(&descriptor)?;
    assert!(
        f.run(job, task, digest, &f.head, None)?.status.success(),
        "working-tree descriptor changed authorized execution"
    );
    for exit in [None, Some("0")] {
        let output = f.run(other, task, digest, &f.head, exit)?;
        assert!(
            !output.status.success(),
            "forged cross-root execution succeeded"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("producer_task_not_in_source"));
    }
    Ok(())
}

fn reject_bad_source(f: &Fixture, job: &str, task: &str, digest: &str) -> TestResult {
    f.write_plan(&f.plan)?;
    for exit in [None, Some("0")] {
        let output = f.run(
            job,
            task,
            digest,
            "0000000000000000000000000000000000000000",
            exit,
        )?;
        assert!(!output.status.success(), "wrong source SHA succeeded");
        assert!(String::from_utf8_lossy(&output.stderr).contains("producer_source_mismatch"));
    }
    let mut descriptor = f.descriptor()?;
    "b3-0000000000000000000000000000000000000000000000000000000000000000"
        .clone_into(&mut descriptor.task_manifest_digest);
    f.write_descriptor(&descriptor)?;
    git(&["add", PRODUCER_EXECUTION_MANIFEST_PATH], f.repo.path())?;
    git(&["commit", "-qm", "bad producer manifest"], f.repo.path())?;
    let head = git_head(f.repo.path())?;
    let mut plan = f.plan.clone();
    plan["head"] = json!(head);
    f.write_plan(&plan)?;
    for exit in [None, Some("0")] {
        let output = f.run(job, task, digest, &head, exit)?;
        assert!(
            !output.status.success(),
            "incorrect committed manifest digest succeeded"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("producer_manifest_binding_mismatch")
        );
    }
    Ok(())
}

fn immutable_manifest_survives_worktree_changes(
    f: &Fixture,
    job: &str,
    task: &str,
    digest: &str,
) -> TestResult {
    let frame_path = f.temp.path().join("resolved-frame");
    let resolve = || -> TestResult<Vec<u8>> {
        let output = f
            .command(job, task, digest, &f.head, None)?
            .env("VELNOR_PRODUCER_TEST_FRAME", &frame_path)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(fs::read(&frame_path)?)
    };
    let baseline = resolve()?;
    let path = f
        .repo
        .path()
        .join(velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH);
    let original = fs::read(&path)?;
    fs::write(&path, "corrupt")?;
    for remove in [false, true] {
        if remove {
            fs::remove_file(&path)?;
        }
        assert_eq!(resolve()?, baseline);
        let report = f.run(job, task, digest, &f.head, Some("0"))?;
        assert!(
            report.status.success(),
            "{}",
            String::from_utf8_lossy(&report.stderr)
        );
    }
    for (name, value, expected) in [
        (
            "GITHUB_SHA",
            "0000000000000000000000000000000000000000",
            "producer_source_mismatch",
        ),
        (
            "GITHUB_EVENT_NAME",
            "pull_request",
            "producer_event_not_authorized",
        ),
        (
            "GITHUB_REF_PROTECTED",
            "false",
            "producer_event_not_authorized",
        ),
    ] {
        let output = f
            .command(job, task, digest, &f.head, None)?
            .env(name, value)
            .output()?;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }
    fs::write(path, original)?;
    Ok(())
}

#[test]
fn producer_tasks_resolve_against_the_real_plan() -> TestResult {
    if std::env::var_os("VELNOR_PRODUCER_TEST_CHILD").is_some() {
        if std::env::var_os("VELNOR_PRODUCER_TEST_REQUEST").is_some() {
            velnor_actions_orchestrator::write_request()?;
        } else if std::env::var_os("VELNOR_PRODUCER_TEST_WRITE").is_some() {
            assert_eq!(velnor_actions_orchestrator::write_task_report()?, 1);
        } else {
            let frame = velnor_actions_orchestrator::resolve_task_execution()?;
            assert!(frame.starts_with(b"VELNOR-TASK-EXECUTION-V1\0"));
            if let Some(path) = std::env::var_os("VELNOR_PRODUCER_TEST_FRAME") {
                fs::write(path, frame)?;
            }
        }
        return Ok(());
    }
    for both in [false, true] {
        let f = Fixture::new(both)?;
        let (job, task, digest, count) = exercise_tasks(&f)?;
        if !both {
            super::producer_execution_limits::event_payload_bounds(&f, &job, &task, &digest)?;
        }
        immutable_manifest_survives_worktree_changes(&f, &job, &task, &digest)?;
        reject_altered_plans(&f, &job, &task, &digest)?;
        reject_forged_membership(&f, &job, &task, &digest)?;
        reject_bad_source(&f, &job, &task, &digest)?;
        assert_eq!(
            fs::read_dir(f.temp.path().join("velnor-producer-reports"))?.count(),
            2 * count + 2 + usize::from(!both)
        );
        assert_eq!(
            fs::read_dir(f.temp.path().join("velnor/r190001-a1"))?.count(),
            1,
            "producer reports must not enter Required artifacts"
        );
        if !both {
            super::producer_execution_limits::replace_ref_attack(&f, &job, &task, &digest)?;
            super::producer_execution_limits::source_artifact_bounds(&f, &job, &task, &digest)?;
        }
    }
    Ok(())
}
