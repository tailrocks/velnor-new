use super::*;

#[test]
fn hosted_and_scale_set_copies_share_one_identical_manifest_record() {
    let selector = scale_set_selector();
    let task = task_step(0);
    let hosted = simple_job(vec![checkout_step(), acquire_step(), task.clone()]);
    let mut scale_set = simple_job(vec![checkout_step(), acquire_step(), task]);
    scale_set.runs_on.clone_from(&selector.token());
    let jobs = BTreeMap::from([
        ("rust-hosted".to_owned(), hosted),
        ("rust-scale-set".to_owned(), scale_set),
    ]);

    let (factored, files) =
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], Some(&selector))
            .expect("both runner lanes use the same typed execution record");
    let manifest = files
        .iter()
        .find(|file| file.path == velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH)
        .expect("shared execution manifest");
    assert_eq!(manifest.bytes.matches("\"task_id\":").count(), 1);
    assert!(
        manifest
            .bytes
            .contains("\"task_id\":\"stack/rust/crate-0/test/default\"")
    );

    let digest_for = |job_id: &str| {
        let StepKind::Action { with, .. } = &factored[job_id].steps[2].kind else {
            panic!("typed task remains an action call in both lanes");
        };
        with.get("digest").expect("manifest record selector")
    };
    assert_eq!(digest_for("rust-hosted"), digest_for("rust-scale-set"));
}

#[test]
fn same_task_id_rejects_a_different_complete_execution_record() {
    let task = task_step(0);
    let mut conflicting_task = task.clone();
    let StepKind::TaskExecution { env, .. } = &mut conflicting_task.kind else {
        unreachable!();
    };
    env.insert("CARGO_TERM_COLOR".to_owned(), "always".to_owned());
    let jobs = BTreeMap::from([
        (
            "rust-first".to_owned(),
            simple_job(vec![checkout_step(), acquire_step(), task]),
        ),
        (
            "rust-second".to_owned(),
            simple_job(vec![checkout_step(), acquire_step(), conflicting_task]),
        ),
    ]);

    let error = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect_err("one task ID cannot select conflicting execution records");
    assert!(
        error
            .to_string()
            .contains("declared_task_id_not_unique:stack/rust/crate-0/test/default")
    );
}

#[test]
fn one_typed_action_serves_150_validated_tasks_without_dropping_job_contracts() {
    let jobs = (0..150)
        .map(|index| {
            let id = format!("rust-crate-{index}");
            let job = Job {
                display_name: format!("Rust / crate {index}"),
                runs_on: "ubuntu-26.04".to_owned(),
                check_runner: None,
                timeout_minutes: JobTimeout::new(20).expect("valid timeout"),
                needs: vec!["plan".to_owned()],
                condition: Some("needs.plan.result == 'success'".to_owned()),
                permissions: None,
                environment: None,
                steps: vec![checkout_step(), acquire_step(), task_step(index)],
            };
            (id, job)
        })
        .collect::<BTreeMap<_, _>>();

    let (factored, files) = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect("factor obligations");

    assert_eq!(factored.len(), 150);
    assert_eq!(
        files.len(),
        2,
        "one shared action and one execution manifest"
    );
    let action_file = files
        .iter()
        .find(|file| file.path.ends_with("/action.yml"))
        .expect("shared composite action");
    let manifest_file = files
        .iter()
        .find(|file| file.path == velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH)
        .expect("versioned task execution manifest");
    assert_eq!(
        action_file.path,
        format!(".github/actions/{ACTION_NAME_PREFIX}0/action.yml")
    );
    assert!(action_file.bytes.contains("using: composite"));
    assert!(action_file.bytes.contains("shell: bash"));
    assert!(manifest_file.bytes.starts_with(&format!(
        "{}\n",
        crate::marker::marker_for_version(VERSION).expect("generator marker")
    )));
    assert!(
        manifest_file
            .bytes
            .contains("\"task_id\":\"stack/rust/crate-0/test/default\"")
    );
    assert!(
        manifest_file
            .bytes
            .contains("crate-0\\\"; printf injected; #")
    );
    assert_eq!(manifest_file.bytes.matches("\"task_id\":").count(), 150);
    assert!(!action_file.bytes.contains("inputs.task_id"));
    assert!(!action_file.bytes.contains("inputs.execution_digest"));
    assert!(action_file.bytes.contains("inputs.digest"));
    let action = task_document_for_test();
    let run = action_run_scalar(&action);
    assert!(run.contains("argv+=(\"$value\")"));
    assert!(run.contains("env -- \"${task_env[@]}\" \"${argv[@]}\""));
    assert!(run.contains("write-task-report-v1"));
    assert!(run.contains("if [ \"$task_code\" -ne 0 ]; then exit \"$task_code\"; fi"));
    assert!(run.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    assert!(!run.contains("eval"));
    assert!(!run.contains("replace_runner_temp"));
    assert!(run.contains("[[ \"$value\" != *'${{'* ]] || fail_frame"));
    assert!(run.contains("VELNOR_RUNTIME_RUNNER_TEMP"));
    for key in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(crate::toolchain_env::STEP_ENDPOINT_DENYLIST)
    {
        assert!(!action_file.bytes.contains(&format!("inputs.env_{key}")));
    }

    for (id, original) in &jobs {
        let rewritten = factored.get(id).expect("job retained");
        assert_eq!(rewritten.display_name, original.display_name);
        assert_eq!(rewritten.runs_on, original.runs_on);
        assert_eq!(rewritten.timeout_minutes, original.timeout_minutes);
        assert_eq!(rewritten.needs, original.needs);
        assert_eq!(rewritten.condition, original.condition);
        assert_eq!(rewritten.permissions, original.permissions);
        assert_eq!(rewritten.environment, original.environment);
        assert_eq!(rewritten.steps.len(), original.steps.len());
        assert_eq!(rewritten.steps[0], original.steps[0]);
        assert_eq!(rewritten.steps[1], original.steps[1]);
        let caller = &rewritten.steps[2];
        assert_eq!(caller.name, original.steps[2].name);
        assert_eq!(caller.condition, original.steps[2].condition);
        let StepKind::Action { uses, with, env } = &caller.kind else {
            panic!("obligation was not replaced by its composite call");
        };
        assert_eq!(uses, "./.github/actions/declared-task-0");
        assert!(
            env.is_empty(),
            "task values cross only through the manifest"
        );
        assert_eq!(
            with.len(),
            1,
            "the full digest selects the validated record"
        );
        assert_eq!(with.keys().next().map(String::as_str), Some("digest"));
        assert_eq!(with["digest"].len(), 67);
        assert!(
            with["digest"].starts_with("b3-")
                && with["digest"][3..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        );
        assert!(!action_file.bytes.contains(HOSTILE_TASK_ARGUMENT));
        assert!(!action_file.bytes.contains("inputs.argv_"));
        assert!(!action_file.bytes.contains("inputs.env_"));
    }
}

#[test]
fn ordinary_shell_and_tofu_steps_stay_unfactored() {
    let shell = crate::steps::shell_step(
        "Tofu remains an ordinary shell step",
        vec!["tofu".to_owned(), "validate".to_owned()],
        BTreeMap::new(),
    )
    .expect("fixed shell step");
    let mut tofu = shell.clone();
    tofu.role = Some(StepRole::TofuProviderUse);
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), shell, tofu]),
    )]);

    let (factored, files) = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect("leave shell tasks alone");
    assert!(files.is_empty());
    assert_eq!(
        factored.keys().collect::<Vec<_>>(),
        jobs.keys().collect::<Vec<_>>()
    );
    for (job_id, original) in &jobs {
        let preserved = &factored[job_id];
        assert_eq!(preserved.display_name, original.display_name);
        assert_eq!(preserved.runs_on, original.runs_on);
        assert_eq!(preserved.check_runner, original.check_runner);
        assert_eq!(preserved.timeout_minutes, original.timeout_minutes);
        assert_eq!(preserved.needs, original.needs);
        assert_eq!(preserved.condition, original.condition);
        assert_eq!(preserved.permissions, original.permissions);
        assert_eq!(preserved.environment, original.environment);
        assert_eq!(preserved.steps, original.steps);
    }
}

#[test]
fn typed_task_requires_checkout_staging_and_credential_free_inputs() {
    let task = task_step(0);
    let no_checkout = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![acquire_step(), task.clone()]),
    )]);
    assert!(
        factor_obligation_steps(&no_checkout, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("checkout is mandatory")
            .to_string()
            .contains("declared_task_requires_checkout")
    );

    let no_stage = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), task.clone()]),
    )]);
    assert!(
        factor_obligation_steps(&no_stage, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("the staged helper is mandatory")
            .to_string()
            .contains("declared_task_requires_staged_helper")
    );

    let mut secret_expression = task.clone();
    let StepKind::TaskExecution { env, .. } = &mut secret_expression.kind else {
        unreachable!();
    };
    env.insert(
        "GITHUB_TOKEN".to_owned(),
        "${{ secrets.GITHUB_TOKEN }}".to_owned(),
    );
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), secret_expression]),
    )]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("only the exact runner.temp expression is supported")
            .to_string()
            .contains("unsupported_declared_task_expression")
    );

    let mut credentialed = task;
    let StepKind::TaskExecution { env, .. } = &mut credentialed.kind else {
        unreachable!();
    };
    env.insert("GITHUB_TOKEN".to_owned(), "fixture-token-value".to_owned());
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), credentialed]),
    )]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("credentials cannot enter task action inputs")
            .to_string()
            .contains("credential_step_env:GITHUB_TOKEN")
    );
}

#[test]
fn typed_task_preserves_only_the_exact_runner_temp_expression() {
    let mut task = task_step(0);
    let StepKind::TaskExecution { env, .. } = &mut task.kind else {
        unreachable!();
    };
    assert_eq!(env["MISE_CARGO_HOME"], "${{ runner.temp }}/velnor/cargo");

    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), task.clone()]),
    )]);
    let (factored, files) = factor_obligation_steps(&jobs, CHECKOUT, VERSION, VERSION, &[], None)
        .expect("known expression is retained in the manifest");
    let manifest = files
        .iter()
        .find(|file| file.path == velnor_actions_contract::TASK_EXECUTION_MANIFEST_PATH)
        .expect("manifest emitted");
    assert!(manifest.bytes.contains("${{ runner.temp }}/velnor/cargo"));
    let StepKind::Action { with, .. } = &factored["rust-demo"].steps[2].kind else {
        panic!("factored task uses the shared action");
    };
    assert_eq!(with.keys().next().map(String::as_str), Some("digest"));
    assert_eq!(with["digest"].len(), 67);

    let StepKind::TaskExecution { env, .. } = &mut task.kind else {
        unreachable!();
    };
    env.insert(
        "MISE_CARGO_HOME".to_owned(),
        "${{ github.workspace }}/.cargo".to_owned(),
    );
    let unsupported = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), task]),
    )]);
    assert!(
        factor_obligation_steps(&unsupported, CHECKOUT, VERSION, VERSION, &[], None)
            .expect_err("unmodeled runtime expressions cannot be serialized")
            .to_string()
            .contains("unsupported_declared_task_expression")
    );
}
