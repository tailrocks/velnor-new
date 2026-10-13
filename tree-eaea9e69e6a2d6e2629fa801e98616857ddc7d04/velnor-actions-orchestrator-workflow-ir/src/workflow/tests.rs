use std::collections::BTreeMap;

use super::*;

#[test]
fn plan_exposes_typed_report_outputs_only_when_upload_is_inserted() {
    let mut plan = Job {
        outputs: Vec::new(),
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: velnor_actions_contract_workflow::JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![velnor_actions_workflow_steps::steps::plan_step()],
    };

    insert_format_report_steps(&mut plan, Vec::new());
    assert_eq!(plan.outputs, Vec::<JobOutput>::new());

    let report = Step {
        name: "Report Format".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let upload = velnor_actions_workflow_steps::crate_job_report_upload_step("plan")
        .expect("typed crate report upload");
    insert_format_report_steps(&mut plan, vec![report, upload]);

    assert_eq!(
        plan.outputs,
        [
            JobOutput::task_report_artifact_id(),
            JobOutput::task_report_check_run_id(),
        ]
    );
    assert_eq!(plan.steps[1].name, "Report Format");
    assert_eq!(plan.steps[2].id, Some(StepId::CrateReportUpload));
}
