// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_rust::is_workspace_fmt_task;
use velnor_actions_workflow_renderer::plan_format::outcome_format_step;
use velnor_actions_workflow_renderer::render::{PLAN_JOB_ID, WORKFLOW_PATH};
use velnor_actions_workflow_renderer::steps::CRATE_REPORT_UPLOAD_NAME;

use super::workflow_configuration_fixture;

#[test]
fn prepare_and_render_emit_workspace_format_with_outcome_and_report_tail()
-> Result<(), Box<dyn Error>> {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\n[[stacks.rust.configurations]]\nname = \"default\"\nfeatures = [\"default\"]\nall_features = false\ntarget = \"host\"\n";
    let repo = workflow_configuration_fixture::make_repo(config)?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    )?;

    let prepared = prepare(root)?;
    let fmt = prepared
        .discovery
        .proposals
        .iter()
        .find(|task| {
            is_workspace_fmt_task(&task.task_kind, &task.identity.unit_id, &task.display_name)
        })
        .ok_or("empty workspace format proposal is missing")?;
    assert_eq!(fmt.stack_id, "rust");
    assert_eq!(fmt.identity.unit_id, "");
    assert_eq!(fmt.display_name, "");
    assert_eq!(fmt.identity.unit_path, "Cargo.toml");

    let jobs = finalized_jobs(&prepared)?;
    let plan = jobs.get(PLAN_JOB_ID).ok_or("Plan job is missing")?;
    let format_steps = plan
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.role == Some(StepRole::PlanFormat))
        .collect::<Vec<_>>();
    assert_eq!(format_steps.len(), 1);
    let (format_at, format) = format_steps[0];
    assert_eq!(format.name, "Format");
    let StepKind::Shell { run, .. } = &format.kind else {
        return Err("Plan Format is not a shell step".into());
    };
    let run = velnor_actions_workflow_renderer::join_argv_for_run(run)?;
    assert!(run.contains("mise"), "workspace formatter command: {run}");
    assert!(run.contains("$RUNNER_TEMP/velnor/outcome-m-"));
    assert!(run.contains("$RUNNER_TEMP/velnor/start-m-"));

    let plan_operation_at = plan
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::PlanProducer))
        .ok_or("Plan producer operation is missing")?;
    let report_at = plan
        .steps
        .iter()
        .position(|step| step.name == "Report Format")
        .ok_or("format report tail is missing")?;
    let upload_at = plan
        .steps
        .iter()
        .position(|step| step.name == CRATE_REPORT_UPLOAD_NAME)
        .ok_or("crate report upload tail is missing")?;
    assert!(format_at < plan_operation_at);
    assert!(plan_operation_at < report_at && report_at < upload_at);

    let rendered = render_staged_tree(&prepared)?;
    let workflow = rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow is missing")?;
    assert!(workflow.contains("name: Format"));
    assert!(workflow.contains("name: Report Format"));
    assert!(workflow.contains(CRATE_REPORT_UPLOAD_NAME));
    Ok(())
}

#[test]
fn workspace_format_outcome_emitter_keeps_native_mise_admission() {
    let error = outcome_format_step(
        vec!["sh".to_owned(), "-c".to_owned(), "mise fmt".to_owned()],
        "m-0123456789abcdef",
        &BTreeMap::new(),
    )
    .expect_err("the outcome wrapper must not bypass native Mise admission");
    assert!(matches!(
        error,
        velnor_actions_workflow_renderer::RenderError::BadCommand(problem)
            if problem == "format_without_mise"
    ));
}
