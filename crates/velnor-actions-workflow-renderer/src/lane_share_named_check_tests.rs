use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
};
use velnor_actions_contract::workflow::lanes::{
    HOSTED_SUFFIX, NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV, SCALE_SUFFIX,
};
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

use super::share_lanes;
use super::tests::{ctx, echo_step, render_jobs, workflow_ir};

const QUALIFICATION_CHECK_CONDITION: &str = "always() && needs.plan.result == 'success'";

fn job(display: &str, runs_on: &str, steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: display.to_owned(),
        runs_on: runs_on.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        check_runner: None,
        steps,
        cache_mode: None,
    }
}

fn named_check_jobs(
    hosted_condition: Option<String>,
    local_condition: Option<String>,
    check_runner: Option<&CheckRunner>,
) -> BTreeMap<String, Job> {
    let mut jobs = BTreeMap::new();
    for (suffix, variant, runs_on, condition) in [
        (HOSTED_SUFFIX, "hosted", "ubuntu-26.04", hosted_condition),
        (
            SCALE_SUFFIX,
            "scale_set",
            "scale-set:velnor+ubuntu-26.04-scale-set",
            local_condition,
        ),
    ] {
        let id = format!("check-demo{suffix}");
        let checkout = Step {
            parallel: false,
            name: "Checkout".to_owned(),
            id: None,
            role: Some(velnor_actions_contract::StepRole::Checkout),
            condition: None,
            kind: StepKind::Action {
                uses: ctx().checkout_uses,
                with: BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]),
                env: BTreeMap::new(),
            },
        };
        let mut steps = vec![checkout, echo_step(0, "shared-preparation")];
        steps.push(Step {
            parallel: false,
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
            parallel: false,
            name: "Upload reports".to_owned(),
            id: None,
            role: Some(velnor_actions_contract::StepRole::MatrixReportUpload),
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
        let mut lane = job(&id, runs_on, steps);
        lane.condition = condition;
        lane.check_runner = check_runner.cloned();
        jobs.insert(id, lane);
    }
    jobs
}

fn linux_hosted_check_runner() -> CheckRunner {
    CheckRunner {
        label: "ubuntu-26.04".to_owned(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::Hosted,
        container: None,
    }
}

#[test]
fn named_check_identity_and_report_upload_stay_on_their_exact_lane() {
    let jobs = named_check_jobs(None, None, None);
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

#[test]
fn typed_named_check_preserves_host_condition_and_lane_admission_when_shared() {
    let local_condition =
        format!("({QUALIFICATION_CHECK_CONDITION}) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})");
    let jobs = named_check_jobs(
        Some(QUALIFICATION_CHECK_CONDITION.to_owned()),
        Some(local_condition.clone()),
        Some(&linux_hosted_check_runner()),
    );

    let shared = share_lanes(&jobs, &ctx()).expect("exactly composed check conditions share");
    assert_eq!(shared.files.len(), 1);
    assert_eq!(
        shared.jobs["check-demo__hosted"].condition.as_deref(),
        Some(QUALIFICATION_CHECK_CONDITION)
    );
    assert_eq!(
        shared.jobs["check-demo__local"].condition.as_deref(),
        Some(local_condition.as_str())
    );
    let yaml = render_jobs(&workflow_ir(), &shared, &ctx()).expect("workflow renders");
    assert!(yaml.contains(QUALIFICATION_CHECK_CONDITION));
    assert!(yaml.contains(&local_condition));
    assert!(yaml.contains("VELNOR_CHECK_JOB_ID: check-demo__hosted"));
    assert!(yaml.contains("VELNOR_CHECK_JOB_ID: check-demo__local"));
    assert!(yaml.contains("name: velnor-crate-run-attempt-check-demo__hosted"));
    assert!(yaml.contains("name: velnor-crate-run-attempt-check-demo__local"));
}

#[test]
fn typed_named_check_keeps_bare_admission_and_rejects_mismatched_composition() {
    let runner = linux_hosted_check_runner();
    let bare = named_check_jobs(
        None,
        Some(EPHEMERAL_CHECK_ADMISSION_CONDITION.to_owned()),
        Some(&runner),
    );
    share_lanes(&bare, &ctx()).expect("missing hosted condition keeps the bare admission");

    let local_condition =
        format!("({QUALIFICATION_CHECK_CONDITION}) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})");
    let mut mismatched = named_check_jobs(
        Some(QUALIFICATION_CHECK_CONDITION.to_owned()),
        Some(local_condition),
        Some(&runner),
    );
    mismatched
        .get_mut("check-demo__local")
        .expect("local named-check lane")
        .condition = Some(format!("(true) && ({EPHEMERAL_CHECK_ADMISSION_CONDITION})"));
    let error = share_lanes(&mismatched, &ctx()).expect_err("mismatched composition fails closed");
    assert!(
        matches!(error, crate::RenderError::InvalidWorkflow(ref problem) if problem == "lane_body_differs:check-demo"),
        "{error}"
    );
}
