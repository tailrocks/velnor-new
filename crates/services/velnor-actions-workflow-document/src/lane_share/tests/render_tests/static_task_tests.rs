use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{
    Job, JobOutput, JobTimeout, Step, StepId, StepKind, StepRole,
};
use velnor_actions_workflow_steps::RenderError;

use super::super::checkout;
use super::{ctx, echo_step, paired, render_jobs, workflow_ir};
use crate::lane_share::{SharedActionInput, share_lanes};

const JOB_ID: &str = "rust-demo";
const TASK_ID: &str = "stack/rust/demo/clippy/default";

fn task_job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Rust demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        outputs: vec![JobOutput::task_report_artifact_id()],
        needs: vec!["plan".to_owned()],
        condition: Some("success()".to_owned()),
        permissions: None,
        environment: None,
        steps,
    }
}

fn coverage_step(condition: &str) -> Step {
    let mut step = echo_step(0, "run obligation");
    step.condition = Some(condition.to_owned());
    step
}

fn upload_step() -> Step {
    Step {
        name: "Upload crate report".to_owned(),
        id: Some(StepId::CrateReportUpload),
        role: Some(StepRole::CrateReportUpload),
        condition: Some("always()".to_owned()),
        kind: StepKind::Action {
            uses: velnor_actions_workflow_steps::steps::UPLOAD_ARTIFACT_USES.to_owned(),
            with: BTreeMap::from([
                ("name".to_owned(), "velnor-crate-report".to_owned()),
                ("path".to_owned(), "reports".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    }
}

fn cache_save_step() -> Step {
    Step {
        name: "Save tools cache".to_owned(),
        id: None,
        role: Some(StepRole::ToolsCacheSave),
        condition: Some("success() && github.event_name == 'push'".to_owned()),
        kind: StepKind::Action {
            uses: "actions/cache/save@0123456789abcdef0123456789abcdef01234567".to_owned(),
            with: BTreeMap::from([
                ("key".to_owned(), "tools-key".to_owned()),
                ("path".to_owned(), ".cache/tools".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    }
}

fn task_job_steps(condition: &str) -> Vec<Step> {
    let mut first = checkout();
    first.name = "Checkout".to_owned();
    vec![
        first,
        echo_step(1, "prepare tools"),
        coverage_step(condition),
        Step {
            condition: Some("always()".to_owned()),
            ..echo_step(2, "always cleanup")
        },
        Step {
            condition: Some("failure()".to_owned()),
            ..echo_step(3, "failure diagnostic")
        },
        cache_save_step(),
        upload_step(),
    ]
}

#[test]
fn static_task_composite_forwards_plan_and_keeps_job_outputs_and_postlude_outer() {
    let jobs = BTreeMap::from([(
        JOB_ID.to_owned(),
        task_job(task_job_steps(&format!(
            "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
        ))),
    )]);
    let context = ctx();
    let shared = share_lanes(&jobs, &context).expect("static task factors");
    let call = &shared.calls[JOB_ID];
    assert_eq!(call.inputs, [SharedActionInput::CoveredTasks]);
    assert_eq!(shared.jobs[JOB_ID].outputs, jobs[JOB_ID].outputs);
    assert_eq!(
        shared.jobs[JOB_ID]
            .steps
            .iter()
            .map(|step| step.role)
            .collect::<Vec<_>>(),
        [
            None,
            None,
            Some(StepRole::ToolsCacheSave),
            Some(StepRole::CrateReportUpload)
        ]
    );

    let workflow = render_jobs(&workflow_ir(), &shared, &context).expect("workflow renders");
    assert!(
        workflow.contains(
            "task_report_artifact_id: ${{ steps.crate-report-upload.outputs.artifact-id }}"
        )
    );
    assert!(workflow.contains("covered_tasks: ${{ needs.plan.outputs.covered_tasks }}"));
    assert!(workflow.contains("if: success()"));
    assert!(workflow.contains("if: success() && github.event_name == 'push'"));
    assert!(workflow.contains("if: always()"));

    let composite = &shared.files[0].bytes;
    assert!(composite.contains("required: true"));
    assert!(
        composite.contains(
            "if: \"!contains(inputs.covered_tasks, ',stack/rust/demo/clippy/default,')\""
        ),
        "{composite}"
    );
    assert!(!composite.contains("if: always()"));
    assert!(!composite.contains("if: failure()"));
    assert!(!composite.contains("needs.plan.outputs.covered_tasks"));
    assert!(!composite.contains("crate-report-upload"));
    assert_eq!(
        shared.files[0].path,
        ".github/actions/task-rust-demo/action.yml"
    );
}

#[test]
fn unpaired_always_and_failure_steps_remain_outer_after_checkout() {
    let jobs = BTreeMap::from([(
        JOB_ID.to_owned(),
        task_job(task_job_steps(&format!(
            "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
        ))),
    )]);
    let context = ctx();
    let shared = share_lanes(&jobs, &context).expect("static task factors");
    assert!(shared.calls.contains_key(JOB_ID));
    let status_conditions = shared.jobs[JOB_ID]
        .steps
        .iter()
        .filter(|step| step.role.is_none())
        .filter_map(|step| step.condition.as_deref())
        .filter(|condition| *condition == "always()" || *condition == "failure()")
        .collect::<Vec<_>>();
    assert_eq!(status_conditions, ["always()", "failure()"]);
    assert!(!shared.files[0].bytes.contains("always()"));
    assert!(!shared.files[0].bytes.contains("failure()"));
}

#[test]
fn report_upload_before_cache_save_keeps_outer_order() {
    let mut steps = task_job_steps(&format!(
        "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
    ));
    steps.swap(5, 6);
    let jobs = BTreeMap::from([(JOB_ID.to_owned(), task_job(steps))]);
    let shared = share_lanes(&jobs, &ctx()).expect("ordered outer postlude factors");
    assert_eq!(
        shared.jobs[JOB_ID]
            .steps
            .iter()
            .map(|step| step.role)
            .collect::<Vec<_>>(),
        [
            None,
            None,
            Some(StepRole::CrateReportUpload),
            Some(StepRole::ToolsCacheSave)
        ]
    );
    assert_eq!(shared.jobs[JOB_ID].outputs, jobs[JOB_ID].outputs);
}

#[test]
fn noncanonical_needs_condition_fails_closed() {
    let jobs = BTreeMap::from([(
        JOB_ID.to_owned(),
        task_job(task_job_steps(&format!(
            "!contains(needs.other.outputs.covered_tasks, ',{TASK_ID},')"
        ))),
    )]);
    let error = share_lanes(&jobs, &ctx()).expect_err("unsupported needs source rejected");
    assert!(matches!(
        error,
        RenderError::InvalidWorkflow(ref detail)
            if detail == "task_composite_unsupported_condition:task-rust-demo"
    ));
}

#[test]
fn noncanonical_needs_reference_in_task_command_fails_closed() {
    let mut steps = task_job_steps(&format!(
        "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
    ));
    steps[1].name = "prepare tools".to_owned();
    let StepKind::Shell { run, .. } = &mut steps[1].kind else {
        panic!("preparation is a shell step");
    };
    run.push("${{ needs.plan.outputs.covered_tasks }}".to_owned());
    let jobs = BTreeMap::from([(JOB_ID.to_owned(), task_job(steps))]);
    let error = share_lanes(&jobs, &ctx()).expect_err("workflow needs is unavailable in composite");
    assert!(
        matches!(
            error,
            RenderError::InvalidWorkflow(ref detail)
                if detail == "task_composite_unsupported_context:prepare tools"
        ),
        "{error:?}"
    );
}

mod context_tests;

#[test]
fn wrong_report_output_binding_prevents_static_task_factor() {
    let mut job = task_job(task_job_steps(&format!(
        "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
    )));
    job.outputs.clear();
    let jobs = BTreeMap::from([(JOB_ID.to_owned(), job)]);
    let error = share_lanes(&jobs, &ctx()).expect_err("output source is mandatory");
    assert!(matches!(
        error,
        RenderError::InvalidWorkflow(ref detail)
            if detail == "task_composite_report_output_mismatch:rust-demo"
    ));
}

#[test]
fn unsafe_task_job_id_cannot_select_a_composite_path() {
    let jobs = BTreeMap::from([(
        "rust-../outside".to_owned(),
        task_job(task_job_steps(&format!(
            "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
        ))),
    )]);
    let error = share_lanes(&jobs, &ctx()).expect_err("unsafe action path rejected");
    assert!(matches!(
        error,
        RenderError::InvalidWorkflow(ref detail)
            if detail == "task_composite_unsafe_job_id:rust-../outside"
    ));
}

#[test]
fn unsupported_postlude_order_is_not_factored() {
    let mut steps = task_job_steps(&format!(
        "!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')"
    ));
    let upload = steps.pop().expect("upload");
    steps.insert(3, upload);
    steps.push(echo_step(4, "late task step"));
    let jobs = BTreeMap::from([(JOB_ID.to_owned(), task_job(steps))]);
    let error = share_lanes(&jobs, &ctx()).expect_err("postlude must be a final suffix");
    assert!(matches!(
        error,
        RenderError::InvalidWorkflow(ref detail)
            if detail == "task_composite_outer_step_order_unsupported:rust-demo"
    ));
}

fn paired_report_jobs(condition: &str) -> BTreeMap<String, Job> {
    let jobs = paired(&[
        echo_step(0, "prepare task"),
        coverage_step(condition),
        upload_step(),
        cache_save_step(),
    ]);
    jobs.into_iter()
        .filter(|(id, _)| id == "rust-0__hosted" || id == "rust-0__local")
        .map(|(id, mut job)| {
            job.needs = vec!["plan".to_owned()];
            job.outputs = vec![
                JobOutput::task_report_artifact_id(),
                JobOutput::task_report_check_run_id(),
            ];
            (id, job)
        })
        .collect()
}

#[test]
fn paired_task_body_reuses_lane_action_and_retains_outer_steps_and_outputs() {
    let condition = format!("!contains(needs.plan.outputs.covered_tasks, ',{TASK_ID},')");
    let mut jobs = paired_report_jobs(&condition);
    super::insert_p08_setup(&mut jobs);
    let context = ctx();
    let shared = share_lanes(&jobs, &context).expect("paired task factors");

    for id in ["rust-0__hosted", "rust-0__local"] {
        assert_eq!(shared.calls[id].uses, "./.github/actions/rust-0");
        assert_eq!(shared.calls[id].inputs, [SharedActionInput::CoveredTasks]);
        assert_eq!(shared.jobs[id].display_name, jobs[id].display_name);
        assert_eq!(shared.jobs[id].runs_on, jobs[id].runs_on);
        assert_eq!(shared.jobs[id].needs, jobs[id].needs);
        assert_eq!(shared.jobs[id].condition, jobs[id].condition);
        assert_eq!(shared.jobs[id].outputs, jobs[id].outputs);
        assert_eq!(shared.checkouts[id], jobs[id].steps[0]);
        assert_eq!(shared.env_steps[id], jobs[id].steps);
        assert_eq!(
            shared.postludes[id]
                .iter()
                .map(|step| step.role)
                .collect::<Vec<_>>(),
            [
                Some(StepRole::CrateReportUpload),
                Some(StepRole::ToolsCacheSave)
            ]
        );
    }
    assert_eq!(
        shared.prefixes["rust-0__hosted"]
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Resolve hosted Mise cache identity",
            "Restore Velnor tool seed",
            "Setup Mise"
        ]
    );
    assert_eq!(
        shared.prefixes["rust-0__local"]
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        ["Setup Mise"]
    );
    assert!(shared.preludes["rust-0__hosted"].is_empty());
    assert!(shared.preludes["rust-0__local"].is_empty());
    assert_eq!(shared.files.len(), 1);
    assert_eq!(shared.files[0].path, ".github/actions/rust-0/action.yml");
    assert!(shared.files[0].bytes.contains("inputs.covered_tasks"));
    assert!(
        !shared.files[0]
            .bytes
            .contains("needs.plan.outputs.covered_tasks")
    );

    let yaml = render_jobs(&workflow_ir(), &shared, &context).expect("workflow renders");
    assert_eq!(yaml.matches("uses: ./.github/actions/rust-0").count(), 2);
    assert_eq!(
        yaml.matches("covered_tasks: ${{ needs.plan.outputs.covered_tasks }}")
            .count(),
        2
    );
    assert_eq!(
        yaml.matches(
            "task_report_artifact_id: ${{ steps.crate-report-upload.outputs.artifact-id }}"
        )
        .count(),
        2
    );
    assert_eq!(
        yaml.matches("task_report_check_run_id: ${{ job.check_run_id }}")
            .count(),
        2
    );
    assert_eq!(yaml.matches("name: Upload crate report").count(), 2);
    assert_eq!(yaml.matches("name: Save tools cache").count(), 2);
}

#[test]
fn paired_task_with_unsupported_needs_source_fails_closed() {
    let jobs = paired_report_jobs(&format!(
        "!contains(needs.other.outputs.covered_tasks, ',{TASK_ID},')"
    ));
    let error = share_lanes(&jobs, &ctx()).expect_err("unsupported task context rejected");
    assert!(matches!(
        error,
        RenderError::InvalidWorkflow(ref detail)
            if detail == "task_composite_unsupported_condition:rust-0"
    ));
}
