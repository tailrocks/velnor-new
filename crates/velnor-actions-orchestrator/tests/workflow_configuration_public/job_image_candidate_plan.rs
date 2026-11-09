// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path, process::Command};

use serde_json::{Value, json};
use velnor_actions_contract::{
    ExecutionMode, JOB_IMAGE_CANDIDATE_JOB_ID, JOB_IMAGE_CANDIDATE_OUTPUTS,
    JOB_IMAGE_CANDIDATE_TASK_ID, expand_workflow_with_lane_plan, named_check_lanes,
};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::scoped_workspace::{commit, plan_request};
use super::workflow_configuration_fixture::git;

fn fixture() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let files = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&source)
        .output()?;
    assert!(files.status.success(), "tracked input inventory failed");
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    git(&["init", "-b", "main"], root)?;
    for path in files
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let relative = std::str::from_utf8(path)?;
        let src = source.join(relative);
        let dst = root.join(relative);
        fs::create_dir_all(dst.parent().ok_or("missing parent")?)?;
        let metadata = fs::symlink_metadata(&src)?;
        if metadata.file_type().is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(&src)?, &dst)?;
        } else if metadata.is_dir() {
            assert_eq!(relative, "qualification/fixtures/submodule");
            assert!(!velnor_actions_contract::native_image_candidate_inputs().contains(&relative));
        } else {
            fs::copy(src, dst)?;
        }
    }
    for relative in [
        "crates/velnor-actions-orchestrator/src/plan_job_image_candidate.rs",
        "crates/velnor-actions-orchestrator/src/job_image_candidate_job.rs",
    ] {
        let destination = root.join(relative);
        if !destination.exists() && source.join(relative).is_file() {
            fs::copy(source.join(relative), destination)?;
        }
    }
    let config = root.join(".velnor/config.toml");
    let text = fs::read_to_string(&config)?;
    let text = text.replace(
        "non_inputs = [\n",
        "non_inputs = [\n  \"docs/planner-fixture/**\",\n",
    );
    fs::write(config, text)?;
    fs::create_dir_all(root.join("docs/planner-fixture"))?;
    fs::write(root.join("docs/planner-fixture/readme.md"), "before\n")?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
        root,
    )?;
    for input in velnor_actions_contract::native_image_candidate_inputs() {
        assert!(
            root.join(input).is_file(),
            "missing canonical source: {input}"
        );
    }
    git(&["add", "-A", "-f"], root)?;
    Ok(dir)
}

fn inventory(root: &Path) -> Result<Value, Box<dyn Error>> {
    let prepared = prepare(root)?;
    let expanded = expand_workflow_with_lane_plan(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    Ok(json!({
        "lane_inventory": expanded.lane_plan.ok_or("missing both lane plan")?.inventory(),
        "named_check_lanes": named_check_lanes(&prepared.workflow.ir, &prepared.config, Some(ExecutionMode::Both))?,
    }))
}

fn request(
    root: &Path,
    base: Option<&str>,
    head: &str,
    event: &str,
    lanes: &Value,
) -> Result<Value, Box<dyn Error>> {
    let mut request = json!({"schema":1,"op":"plan-v1","run_key":"r190001-a1","base":base,"head":head,"event":event,"root":root.to_string_lossy(),"lane_inventory":lanes["lane_inventory"],"named_check_lanes":lanes["named_check_lanes"]});
    if event == "qualification" {
        request["repository"] = json!("tailrocks/velnor-new");
        request["qualification"] = json!({"campaign":"fixture","phase":"cold","repository":"tailrocks/velnor-new","default_branch":"main","git_ref":"refs/heads/main","ref_protected":true,"workflow_ref":"tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main","workflow_sha":head,"source_sha":head,"run_id":190_001,"run_attempt":1});
    }
    Ok(plan_request(&request)?.value)
}

fn assert_candidate(value: &Value, decision: &str) -> Result<(), Box<dyn Error>> {
    let plan = &value["plan"];
    let obligations: Vec<_> = plan["obligations"]
        .as_array()
        .ok_or("missing obligations")?
        .iter()
        .filter(|item| item["task_id"] == JOB_IMAGE_CANDIDATE_TASK_ID)
        .collect();
    assert_eq!(
        obligations.len(),
        1,
        "candidate obligation missing or duplicated"
    );
    assert_eq!(obligations[0]["job_id"], JOB_IMAGE_CANDIDATE_JOB_ID);
    assert_eq!(obligations[0]["decision"], decision);
    let jobs: Vec<_> = plan["jobs"]
        .as_array()
        .ok_or("missing jobs")?
        .iter()
        .filter(|item| item["job_id"] == JOB_IMAGE_CANDIDATE_JOB_ID)
        .collect();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        jobs[0]["disposition"],
        if decision == "execute" {
            "run"
        } else {
            "not_affected"
        }
    );
    let entries: Vec<_> = plan["matrix"]["include"]
        .as_array()
        .ok_or("missing matrix")?
        .iter()
        .filter(|item| item["task_id"] == JOB_IMAGE_CANDIDATE_TASK_ID)
        .collect();
    if decision == "execute" {
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["job_id"], JOB_IMAGE_CANDIDATE_JOB_ID);
        assert_eq!(
            entries[0]["declared_outputs"],
            json!(JOB_IMAGE_CANDIDATE_OUTPUTS)
        );
    } else {
        assert_eq!(entries, [] as [&Value; 0]);
    }
    Ok(())
}

#[test]
fn image_candidate_follows_public_plan_and_full_events() -> Result<(), Box<dyn Error>> {
    let dir = fixture()?;
    let root = dir.path();
    let rendered = render_staged_tree(&prepare(root)?)?;
    assert!(
        rendered
            .get(".github/workflows/job-image-candidate.yml")
            .is_none(),
        "native job image must not emit a standalone builder"
    );
    let publish = rendered
        .get(".github/workflows/publish.yml")
        .ok_or("missing native publication workflow")?;
    assert!(publish.contains("job-image-candidate-attest:"));
    assert!(publish.contains("job-image-publish:"));
    let base = commit(root, "fixture base")?;
    let candidate = root.join("images/job/ubuntu-26.04/build-test.sh");
    let original = fs::read(&candidate)?;
    let mut edited = original.clone();
    edited.push(b'\n');
    fs::write(&candidate, edited)?;
    let relevant = commit(root, "candidate input changed")?;
    let lanes = inventory(root)?;
    assert_candidate(
        &request(root, Some(&base), &relevant, "pull_request", &lanes)?,
        "execute",
    )?;
    fs::write(candidate, original)?;
    fs::write(root.join("docs/planner-fixture/readme.md"), "after\n")?;
    let docs = commit(root, "unrelated documentation")?;
    let lanes = inventory(root)?;
    assert_candidate(
        &request(root, Some(&base), &docs, "pull_request", &lanes)?,
        "not_affected",
    )?;
    assert_candidate(
        &request(root, None, &docs, "pull_request", &lanes)?,
        "execute",
    )?;
    for event in ["push", "merge_group", "qualification"] {
        assert_candidate(&request(root, None, &docs, event, &lanes)?, "execute")?;
    }
    let test_source = "crates/velnor-actions-orchestrator/src/lock_audit_tests.rs";
    assert!(!velnor_actions_contract::native_image_candidate_inputs().contains(&test_source));
    let path = root.join(test_source);
    fs::write(&path, [fs::read(&path)?, vec![b'\n']].concat())?;
    let test_change = commit(root, "test-only source changed")?;
    let lanes = inventory(root)?;
    assert_candidate(
        &request(root, Some(&docs), &test_change, "pull_request", &lanes)?,
        "not_affected",
    )?;
    let new_source = root.join("crates/velnor-actions-orchestrator/src/candidate_new_unique.rs");
    assert!(!new_source.exists());
    fs::write(new_source, "pub fn candidate_new_unique() {}\n")?;
    let added_source = commit(root, "new potential source")?;
    let lanes = inventory(root)?;
    assert_candidate(
        &request(
            root,
            Some(&test_change),
            &added_source,
            "pull_request",
            &lanes,
        )?,
        "execute",
    )?;
    let content = fs::read_to_string(root.join(".velnor/config.toml"))?;
    assert!(content.contains("compiler_process_budget = 2"));
    for (configured, expected) in [(1, "1"), (65, "64")] {
        fs::write(
            root.join(".velnor/config.toml"),
            content.replace(
                "compiler_process_budget = 2",
                &format!("compiler_process_budget = {configured}"),
            ),
        )?;
        let prepared = prepare(root)?;
        let candidate = &prepared.workflow.ir.jobs[JOB_IMAGE_CANDIDATE_JOB_ID];
        let env = candidate
            .steps
            .iter()
            .find_map(|step| match &step.kind {
                velnor_actions_contract::StepKind::TaskExecution { env, .. } => Some(env),
                _ => None,
            })
            .ok_or("missing candidate execution")?;
        assert_eq!(
            env.get("CARGO_BUILD_JOBS").map(String::as_str),
            Some(expected)
        );
    }
    Ok(())
}
