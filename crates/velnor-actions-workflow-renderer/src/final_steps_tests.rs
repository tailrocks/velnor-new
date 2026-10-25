use super::*;
use crate::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP};
use velnor_actions_contract::{
    Concurrency, JobTimeout, PermissionLevel, Permissions, StepKind, Trigger, WorkflowIr,
};

#[test]
fn required_job_fetches_then_writes_and_conditionally_uploads_qualification_receipt() {
    let (mut jobs, ctx) = required_job_fixture();
    insert_final_fanin(&mut jobs, &ctx).expect("final fan-in");
    let job = jobs.get(FINAL_JOB_ID).expect("required job");
    assert_receipt_step_order_and_rendering(job, &ctx);
    let yaml = render_required_workflow(job, &ctx);
    assert_eq!(
        yaml.matches("if: steps.qualification-cache-receipt.outputs.receipt_path != ''")
            .count(),
        1,
        "the upload uses the exact standalone GitHub condition"
    );
    assert!(
        !yaml.contains("if: ${{ steps.qualification-cache-receipt.outputs.receipt_path }} != ''"),
        "the expression is not split around GitHub's expression delimiters"
    );
    if let Some(path) = std::env::var_os("VELNOR_TEST_RENDERED_WORKFLOW_PATH") {
        std::fs::write(path, yaml).expect("write requested generated-workflow evidence");
    }
}

fn required_job_fixture() -> (BTreeMap<String, Job>, RenderContext) {
    let job = Job {
        display_name: "Required".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: vec![crate::render::PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        services: vec![],
        steps: vec![
            crate::closure::download_plan_step().expect("download plan"),
            steps::merge_step(),
        ],
    };
    let ctx = RenderContext {
        tool_pins: Vec::new(),
        generator_version: "test".to_owned(),
        report_helper_version: "test".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        staged_binary: "binary".to_owned(),
        verified_cache_helpers: BTreeMap::new(),
        require_verified_cache_helper: false,
        request_dir: "requests".to_owned(),
        checkout_uses: steps::CHECKOUT_USES.to_owned(),
        validator_commands: vec![],
        candidate: None,
        preseed: false,
        workflow_tasks: vec![],
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::default(),
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    };
    (
        BTreeMap::from([
            (crate::render::PLAN_JOB_ID.to_owned(), plan_job_fixture()),
            (FINAL_JOB_ID.to_owned(), job),
        ]),
        ctx,
    )
}

fn assert_receipt_step_order_and_rendering(job: &Job, ctx: &RenderContext) {
    let roles = job.steps.iter().map(|step| step.role).collect::<Vec<_>>();
    let fetch = roles
        .iter()
        .position(|role| *role == Some(StepRole::FetchReports))
        .expect("fetch reports");
    assert_eq!(roles[fetch + 1], Some(StepRole::QualificationCacheReceipt));
    assert_eq!(
        roles[fetch + 2],
        Some(StepRole::QualificationCacheReceiptUpload)
    );
    let StepKind::Internal { env, .. } = &job.steps[fetch].kind else {
        panic!("fetch is internal");
    };
    assert!(env.is_empty(), "fetch auth is injected by the renderer");
    let upload = &job.steps[fetch + 2];
    assert_eq!(
        upload.condition.as_deref(),
        Some(QUALIFICATION_CACHE_RECEIPT_UPLOAD_CONDITION)
    );
    let StepKind::Action { with, .. } = &upload.kind else {
        panic!("receipt upload is an action");
    };
    assert_eq!(
        with.get("if-no-files-found").map(String::as_str),
        Some("error")
    );
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(QUALIFICATION_CACHE_RECEIPT_PATH_EXPR)
    );
    let step_context = crate::document_lanes::JobStepContext {
        job_env: &BTreeMap::new(),
        runs_on: Some("ubuntu-24.04"),
        actions_read: true,
        relocated_child: false,
    };
    let rendered_fetch = crate::document_steps::step_to_yaml(
        FINAL_JOB_ID,
        &job.steps[fetch],
        ctx,
        &[],
        false,
        &step_context,
    )
    .expect("fetch renders with fixed token binding");
    assert!(format!("{rendered_fetch:?}").contains("GH_TOKEN"));
    let rendered_upload =
        crate::document_steps::step_to_yaml(FINAL_JOB_ID, upload, ctx, &[], false, &step_context)
            .expect("conditional upload renders");
    assert!(format!("{rendered_upload:?}").contains("qualification-cache-receipt"));
}

fn render_required_workflow(job: &Job, ctx: &RenderContext) -> String {
    let jobs = BTreeMap::from([
        (crate::render::PLAN_JOB_ID.to_owned(), plan_job_fixture()),
        (FINAL_JOB_ID.to_owned(), job.clone()),
    ]);
    let shared = crate::lane_share::share_lanes(&jobs, ctx).expect("lane sharing");
    let workflow = WorkflowIr {
        name: "Required receipt test".to_owned(),
        triggers: Trigger {
            pull_request_types: vec!["opened".to_owned()],
            push_branches: vec!["main".to_owned()],
            merge_group: false,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions {
            actions: PermissionLevel::Read,
            ..Permissions::default()
        },
        concurrency: Concurrency {
            group: CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
        },
        jobs,
        cache_mode: None,
    };
    let rendered = crate::document::workflow_to_yaml(
        &workflow,
        &shared,
        ctx,
        &std::collections::BTreeSet::new(),
    )
    .expect("Required workflow renders");
    crate::yaml::render_yaml(&rendered)
}

fn plan_job_fixture() -> Job {
    Job {
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: vec![],
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        services: vec![],
        steps: vec![steps::plan_step()],
    }
}
