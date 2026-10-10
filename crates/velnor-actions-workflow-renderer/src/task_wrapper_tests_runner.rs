use super::*;

#[test]
fn typed_task_rejects_helper_version_from_a_different_generation_context() {
    let mut task = task_step(0);
    let StepKind::TaskExecution {
        report_helper_version,
        ..
    } = &mut task.kind
    else {
        unreachable!();
    };
    *report_helper_version = "0.1.4".to_owned();
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), task]),
    )]);

    let error = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect_err("task metadata cannot name a different helper release than the renderer");
    assert!(
        error
            .to_string()
            .contains("declared_task_helper_version_mismatch:rust-demo"),
        "unexpected error: {error}"
    );
}

#[test]
fn generated_task_action_marker_uses_generator_version_and_body_uses_helper_version() {
    const CONSUMER_HELPER_VERSION: &str = "0.1.4";
    let mut task = task_step(0);
    let StepKind::TaskExecution {
        report_helper_version,
        ..
    } = &mut task.kind
    else {
        unreachable!();
    };
    *report_helper_version = CONSUMER_HELPER_VERSION.to_owned();
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![
            checkout_step(),
            acquire_step_for(CONSUMER_HELPER_VERSION),
            task,
        ]),
    )]);

    let (_, files) =
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, CONSUMER_HELPER_VERSION, &[], None)
            .expect("factor task under independently versioned generator/helper");
    assert_eq!(files.len(), 2);
    let action = files
        .iter()
        .find(|file| file.path.ends_with("/action.yml"))
        .expect("action output");
    let manifest = files
        .iter()
        .find(|file| file.path == velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH)
        .expect("manifest output");
    assert!(action.bytes.starts_with(&format!(
        "{}\n",
        crate::marker::marker_for_version(VERSION).expect("generator marker")
    )));
    assert!(
        action
            .bytes
            .contains("$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.4")
    );
    assert!(manifest.bytes.starts_with(&format!(
        "{}\n",
        crate::marker::marker_for_version(VERSION).expect("generator marker")
    )));
    assert!(
        manifest
            .bytes
            .contains("\"report_helper_version\":\"0.1.4\"")
    );
}

#[test]
fn scale_set_task_requires_a_resolved_linux_x64_verification_profile() {
    let mut job = simple_job(vec![checkout_step(), acquire_step(), task_step(0)]);
    job.runs_on = "scale-set:velnor+orbstack-linux".to_owned();
    let jobs = BTreeMap::from([("rust-demo".to_owned(), job)]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("an arbitrary self-hosted selector does not prove linux/amd64")
            .to_string()
            .contains("declared_task_requires_supported_linux_runner")
    );
}

#[test]
fn scale_set_task_rejects_a_non_linux_profile_or_different_resolved_token() {
    let selector = scale_set_selector();
    let linux = verification_policy(VerificationRunner::LinuxX64);
    assert!(super::super::supports_task_runner(
        "task-rust-demo",
        "scale-set:velnor+ubuntu-26.04-scale-set",
        std::slice::from_ref(&linux),
        Some(&selector),
    ));

    let macos = verification_policy(VerificationRunner::MacosArm64);
    assert!(!super::super::supports_task_runner(
        "task-rust-demo",
        "scale-set:velnor+ubuntu-26.04-scale-set",
        std::slice::from_ref(&macos),
        Some(&selector),
    ));
    assert!(!super::super::supports_task_runner(
        "task-rust-demo",
        "scale-set:velnor+orbstack-linux",
        std::slice::from_ref(&linux),
        Some(&selector),
    ));
}

#[test]
fn crate_obligation_scale_set_uses_the_explicit_profile_without_a_task_policy() {
    let selector = scale_set_selector();
    let token = selector.token();
    let mut job = simple_job(vec![checkout_step(), acquire_step(), task_step(0)]);
    job.runs_on.clone_from(&token);
    let jobs = BTreeMap::from([("rust-demo".to_owned(), job)]);

    assert!(factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None).is_err());
    let (_, files) =
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], Some(&selector))
            .expect("validated execution profile authorizes the crate route");
    assert_eq!(files.len(), 2);
}

#[test]
fn crate_obligation_scale_set_rejects_a_different_resolved_profile() {
    let selector = scale_set_selector();
    let mut job = simple_job(vec![checkout_step(), acquire_step(), task_step(0)]);
    job.runs_on = "scale-set:velnor+orbstack-linux".to_owned();
    let jobs = BTreeMap::from([("rust-demo".to_owned(), job)]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], Some(&selector)).is_err()
    );
}
