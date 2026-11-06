// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use serde_json::Value;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::super::workflow_configuration_fixture;
use super::{check_generated_workflow_runners, parsed, rendered_files};

fn arm_task_repo() -> Result<tempfile::TempDir, Box<dyn Error>> {
    let config = "schema = 1\n\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\ngenerator_validation = \"bootstrap\"\n\n[[workflow.tasks]]\nid = \"arm-image\"\nkind = \"native-image\"\nplatform = \"linux-arm64\"\nscript = \"maintained-image-build/probe.sh\"\ntimeout_minutes = 5\ncache = \"task-owned-builder\"\n";
    let repo = workflow_configuration_fixture::make_repo(config)?;
    let script = repo.path().join("maintained-image-build/probe.sh");
    fs::create_dir_all(script.parent().ok_or("ARM task script has no parent")?)?;
    fs::write(&script, "set -eu\nprintf 'arm task fixture\\n'\n")?;
    workflow_configuration_fixture::git(&["add", "maintained-image-build/probe.sh"], repo.path())?;
    Ok(repo)
}

#[test]
fn explicit_linux_arm_task_uses_the_ubuntu_26_04_arm_runner() -> Result<(), Box<dyn Error>> {
    let repo = arm_task_repo()?;
    let files = rendered_files(&render_staged_tree(&prepare(repo.path())?)?)?;
    let ci_path = ".github/workflows/ci.yml";
    let ci = parsed(
        files
            .get(ci_path)
            .ok_or("ARM task CI workflow was not staged")?,
        ci_path,
    )?;
    let arm_job = ci
        .get("jobs")
        .and_then(|jobs| jobs.get("task-arm-image"))
        .ok_or("explicit ARM task was not rendered")?;
    if arm_job.get("runs-on").and_then(Value::as_str) != Some("ubuntu-26.04-arm") {
        return Err("explicit linux-arm64 task did not select ubuntu-26.04-arm".into());
    }
    let plan = ci
        .get("jobs")
        .and_then(|jobs| jobs.get("plan"))
        .ok_or("ARM task CI plan job was not rendered")?;
    if plan.get("runs-on").and_then(Value::as_str) != Some("ubuntu-26.04") {
        return Err("ARM task changed the default x64 runner".into());
    }
    let invocation = arm_job
        .get("steps")
        .and_then(Value::as_array)
        .and_then(|steps| {
            steps.iter().find(|step| {
                step.get("name").and_then(Value::as_str) == Some("Run native image validation")
            })
        })
        .and_then(|step| step.get("run"))
        .and_then(Value::as_str)
        .ok_or("ARM task invocation was not rendered")?;
    if !invocation.trim_end().ends_with("linux/arm64") {
        return Err(format!("ARM invocation did not select linux/arm64: {invocation}").into());
    }
    check_generated_workflow_runners(&files)?;
    Ok(())
}
