use velnor_actions_contract::{Step, StepKind, WorkflowPolicy};
use velnor_actions_workflow_renderer::cache_p08::{
    ensure_setup_p08, infer_job_tools, mise_cache_key_for_tools,
};
use velnor_actions_workflow_renderer::steps::{
    MBX_PREFLIGHT_NAME, TOOLS_RESTORE_NAME, mbx_steps_for_driver,
};
use velnor_actions_workflow_renderer::{finalize_jobs, render_workflow_ir_strict};

use super::super::super::impl_renderer_fixtures::{
    TEST_MBX_VERSION, TEST_RUST_TOOLCHAIN, acquire_fixture, checkout_pin, fixture_ctx, fixture_ir,
    job, mbx_tool_env, minimal_plan_job, mise, scrubbed_shell_step,
};

const MBX_ACTION: &str = "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TARGET: &str = "x86_64-unknown-linux-gnu";

fn emitted_pair() -> [Step; 2] {
    mbx_steps_for_driver(
        MBX_ACTION,
        velnor_actions_workflow_renderer::steps::CompileDriver::Mbx,
        TEST_MBX_VERSION,
        TEST_RUST_TOOLCHAIN,
        mbx_tool_env(TEST_RUST_TOOLCHAIN),
    )
    .expect("valid MBX lane")
    .expect("MBX lane emits its preflight and action")
}

fn lane_job(preflight: Step, action: Step) -> (String, velnor_actions_contract::Job) {
    job(
        "mbx-lane",
        "MBX lane",
        vec!["plan".to_owned()],
        vec![
            velnor_actions_workflow_renderer::steps::checkout_step(&checkout_pin())
                .expect("checkout"),
            preflight,
            action,
        ],
    )
}

#[test]
fn actual_emitted_mbx_lane_gets_a_key_for_its_exact_pins() {
    let [preflight, action] = emitted_pair();
    let (_, lane) = lane_job(preflight, action);
    let expected_specs = vec![
        format!("mr-boxington@{TEST_MBX_VERSION}"),
        format!("rust@{TEST_RUST_TOOLCHAIN}"),
    ];
    assert_eq!(infer_job_tools(&lane), expected_specs);

    let mut plan = minimal_plan_job().expect("plan job");
    plan.1
        .steps
        .insert(1, acquire_fixture().expect("Acquire Velnor"));
    let ir = fixture_ir(vec![plan, ("mbx-lane".to_owned(), lane)]);
    let jobs = finalize_jobs(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )
    .expect("the emitted MBX job finalizes");
    let mbx_job = jobs.get("mbx-lane").expect("finalized MBX job");
    assert_eq!(infer_job_tools(mbx_job), expected_specs);

    let mut changed_export = mbx_job.clone();
    let export = changed_export
        .steps
        .iter_mut()
        .find(|step| step.name == "Export MBX single bundle")
        .expect("generated MBX export step");
    let StepKind::Shell { run, .. } = &mut export.kind else {
        panic!("generated MBX export step is a shell step");
    };
    run[2].push_str("; mise install gh@latest");
    assert!(
        ensure_setup_p08("mbx-lane", &mut changed_export, &mise(), false, TARGET).is_err(),
        "a changed generated export script must not bypass tool inference"
    );

    let restore = mbx_job
        .steps
        .iter()
        .find(|step| step.name == TOOLS_RESTORE_NAME)
        .expect("exact tool-cache restore");
    let StepKind::Action { with, .. } = &restore.kind else {
        panic!("tool restore is an action");
    };
    let expected_key = mise_cache_key_for_tools(TARGET, "2026.9.18", &expected_specs)
        .expect("exact tool-cache key");
    assert_eq!(with.get("key"), Some(&expected_key));
    let yaml = render_workflow_ir_strict(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )
    .expect("workflow strictly renders");
    assert!(yaml.contains(MBX_PREFLIGHT_NAME));
    assert!(yaml.contains(&expected_key));
}

#[test]
fn mbx_preflight_identity_rejects_name_condition_and_body_changes() {
    let [preflight, action] = emitted_pair();

    let mut renamed = preflight.clone();
    renamed.name = "Other step".to_owned();
    assert_rejected(renamed, action.clone());

    let mut changed_condition = preflight.clone();
    changed_condition.condition = Some("always()".to_owned());
    assert_rejected(changed_condition, action.clone());

    let mut changed_body = preflight.clone();
    let StepKind::Shell { run, .. } = &mut changed_body.kind else {
        panic!("preflight is a shell step");
    };
    run[2].push_str("; echo changed");
    assert_rejected(changed_body, action.clone());

    let mut arbitrary = preflight;
    let StepKind::Shell { run, .. } = &mut arbitrary.kind else {
        panic!("preflight is a shell step");
    };
    run[2] = "if true; then mise where gh@2.88.0; fi".to_owned();
    assert_rejected(arbitrary, action);
}

#[test]
fn mbx_preflight_identity_rejects_noncanonical_env_and_scrub_changes() {
    let [preflight, action] = emitted_pair();

    let mut changed_env = preflight.clone();
    let StepKind::Shell { env, .. } = &mut changed_env.kind else {
        panic!("preflight is a shell step");
    };
    env.insert(
        "MISE_DATA_DIR".to_owned(),
        "${{ runner.temp }}/other-mise".to_owned(),
    );
    assert_rejected(changed_env, action.clone());

    let mut changed_pair_preflight = preflight.clone();
    let mut changed_pair_action = action.clone();
    let StepKind::Shell { env, .. } = &mut changed_pair_preflight.kind else {
        panic!("preflight is a shell step");
    };
    env.insert(
        "MISE_DATA_DIR".to_owned(),
        "${{ runner.temp }}/other-mise".to_owned(),
    );
    let StepKind::Action { env, .. } = &mut changed_pair_action.kind else {
        panic!("MBX restore is an action");
    };
    env.insert(
        "MISE_DATA_DIR".to_owned(),
        "${{ runner.temp }}/other-mise".to_owned(),
    );
    assert_rejected(changed_pair_preflight, changed_pair_action);

    let mut changed_native_home_preflight = preflight.clone();
    let mut changed_native_home_action = action.clone();
    let StepKind::Shell { env, .. } = &mut changed_native_home_preflight.kind else {
        panic!("preflight is a shell step");
    };
    env.insert(
        "RUSTUP_HOME".to_owned(),
        "${{ runner.temp }}/other-rustup".to_owned(),
    );
    let StepKind::Action { env, .. } = &mut changed_native_home_action.kind else {
        panic!("MBX restore is an action");
    };
    env.insert(
        "RUSTUP_HOME".to_owned(),
        "${{ runner.temp }}/other-rustup".to_owned(),
    );
    assert_rejected(changed_native_home_preflight, changed_native_home_action);

    let mut missing_scrub = preflight.clone();
    let StepKind::Shell { env, .. } = &mut missing_scrub.kind else {
        panic!("preflight is a shell step");
    };
    env.remove("GITHUB_TOKEN");
    assert_rejected(missing_scrub, action.clone());

    let mut changed_scrub = preflight.clone();
    let StepKind::Shell { env, .. } = &mut changed_scrub.kind else {
        panic!("preflight is a shell step");
    };
    env.insert("GITHUB_TOKEN".to_owned(), "untrusted".to_owned());
    assert_rejected(changed_scrub, action.clone());
}

#[test]
fn mbx_preflight_identity_rejects_action_and_neighbor_changes() {
    let [preflight, action] = emitted_pair();
    let mut changed_action = action.clone();
    let StepKind::Action { with, .. } = &mut changed_action.kind else {
        panic!("MBX restore is an action");
    };
    with.insert("version".to_owned(), "latest".to_owned());
    assert_rejected(preflight.clone(), changed_action);

    let mut floating_action = action.clone();
    let StepKind::Action { uses, .. } = &mut floating_action.kind else {
        panic!("MBX restore is an action");
    };
    *uses = "jdx/mr-boxington-action@main".to_owned();
    assert_rejected(preflight.clone(), floating_action);

    let mut duplicate = lane_job(preflight.clone(), action.clone()).1;
    duplicate.steps.push(action.clone());
    assert_rejected_job(duplicate);

    let mut nonadjacent = lane_job(preflight, action.clone()).1;
    nonadjacent.steps.insert(
        2,
        scrubbed_shell_step("Between MBX steps", vec!["echo".into(), "ok".into()])
            .expect("safe shell step"),
    );
    assert_rejected_job(nonadjacent);
}

fn assert_rejected(preflight: Step, action: Step) {
    assert_rejected_job(lane_job(preflight, action).1);
}

fn assert_rejected_job(mut lane: velnor_actions_contract::Job) {
    assert!(
        infer_job_tools(&lane)
            .iter()
            .any(|spec| spec.contains("unsupported")),
        "an unrecognized generated step must stay visible as an invalid tool"
    );
    assert!(
        ensure_setup_p08("mbx-lane", &mut lane, &mise(), false, TARGET).is_err(),
        "an unrecognized MBX preflight must not create a cache key"
    );
}
