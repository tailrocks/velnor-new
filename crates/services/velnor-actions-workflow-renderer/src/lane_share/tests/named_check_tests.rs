use std::collections::BTreeMap;

use velnor_actions_contract_workflow::workflow::lanes::{
    HOSTED_SUFFIX, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, SCALE_SUFFIX,
};
use velnor_actions_contract_workflow::{Job, JobTimeout, Step, StepKind, StepRole};

use super::super::share_lanes;
use super::{ctx, echo_step, render_jobs, workflow_ir};

fn job(display: &str, runs_on: &str, steps: Vec<Step>) -> Job {
    Job {
        display_name: display.to_owned(),
        runs_on: runs_on.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        check_runner: None,
        steps,
    }
}

#[test]
fn named_check_identity_and_report_upload_stay_on_their_exact_lane() {
    let mut jobs = BTreeMap::new();
    for (suffix, variant, runs_on) in [
        (HOSTED_SUFFIX, "hosted", "ubuntu-26.04"),
        (
            SCALE_SUFFIX,
            "scale_set",
            "scale-set:velnor+ubuntu-26.04-scale-set",
        ),
    ] {
        let id = format!("check-demo{suffix}");
        let checkout = Step {
            name: "Checkout".to_owned(),
            id: None,
            role: Some(StepRole::Checkout),
            condition: None,
            kind: StepKind::Action {
                uses: ctx().checkout_uses,
                with: BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]),
                env: BTreeMap::new(),
            },
        };
        let mut steps = vec![checkout, echo_step(0, "shared-preparation")];
        steps.push(Step {
            name: "Execute named check".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Shell {
                run: vec!["velnor-actions".to_owned()],
                env: BTreeMap::from([
                    (NAMED_CHECK_JOB_ID_ENV.to_owned(), id.clone()),
                    (NAMED_CHECK_LANE_VARIANT_ENV.to_owned(), variant.to_owned()),
                ]),
            },
        });
        steps.push(Step {
            name: "Upload reports".to_owned(),
            id: None,
            role: Some(StepRole::MatrixReportUpload),
            condition: Some("always()".to_owned()),
            kind: StepKind::Action {
                uses: crate::steps::UPLOAD_ARTIFACT_USES.to_owned(),
                with: BTreeMap::from([
                    ("name".to_owned(), format!("velnor-crate-run-attempt-{id}")),
                    ("path".to_owned(), "reports".to_owned()),
                ]),
                env: BTreeMap::new(),
            },
        });
        jobs.insert(id.clone(), job(&id, runs_on, steps));
    }

    let shared = share_lanes(&jobs, &ctx()).expect("lane-specific steps are retained");
    assert_eq!(shared.files.len(), 1);
    assert!(!shared.files[0].bytes.contains("VELNOR_CHECK_JOB_ID"));
    assert_eq!(shared.jobs["check-demo__hosted"].steps.len(), 2);
    assert_eq!(shared.jobs["check-demo__local"].steps.len(), 2);
    let yaml = render_jobs(&workflow_ir(), &shared, &ctx()).expect("workflow renders");
    assert!(yaml.contains("VELNOR_CHECK_JOB_ID: check-demo__hosted"));
    assert!(yaml.contains("VELNOR_CHECK_JOB_ID: check-demo__local"));
    assert!(yaml.contains("name: velnor-crate-run-attempt-check-demo__hosted"));
    assert!(yaml.contains("name: velnor-crate-run-attempt-check-demo__local"));
}
