use std::collections::BTreeMap;

use velnor_actions_contract::cachekey::{ToolchainInputs, toolchain_id};
use velnor_actions_contract::workflow::crate_job::task_digest_for_execution;
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};

use crate::yaml::Yaml;

use super::{ACTION_NAME_PREFIX, factor_obligation_steps};

const CHECKOUT: &str = "actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const VERSION: &str = "0.1.6";

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

    let (factored, files) =
        factor_obligation_steps(&jobs, CHECKOUT, VERSION).expect("factor obligations");

    assert_eq!(factored.len(), 150);
    assert_eq!(files.len(), 1, "argv/env shape should share one action");
    assert_eq!(
        files[0].path,
        format!(".github/actions/{ACTION_NAME_PREFIX}0/action.yml")
    );
    assert!(files[0].bytes.contains("using: composite"));
    assert!(files[0].bytes.contains("shell: bash"));
    let action = task_document_for_test();
    let run = action_run_scalar(&action);
    assert!(run.contains("argv=( \"$VELNOR_WRAPPER_ARGV_0\""));
    assert!(run.contains("write-task-report-v1"));
    assert!(run.contains("if [ \"$task_code\" -ne 0 ]; then exit \"$task_code\"; fi"));
    assert!(run.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    assert!(!run.contains("eval"));
    for key in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(crate::toolchain_env::STEP_ENDPOINT_DENYLIST)
    {
        assert!(!files[0].bytes.contains(&format!("inputs.env_{key}")));
    }

    for (id, original) in &jobs {
        let task_index = id.strip_prefix("rust-crate-").expect("numeric crate ID");
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
        assert!(env.is_empty(), "task env crosses through declared inputs");
        assert_eq!(with["argv_10"], format!("crate-{task_index}"));
        assert_eq!(
            with["task_id"],
            format!("stack/rust/crate-{task_index}/test/default")
        );
        assert!(!with.contains_key("env_GITHUB_TOKEN"));
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

    let (factored, files) =
        factor_obligation_steps(&jobs, CHECKOUT, VERSION).expect("leave shell tasks alone");
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
        factor_obligation_steps(&no_checkout, CHECKOUT, VERSION)
            .expect_err("checkout is mandatory")
            .to_string()
            .contains("declared_task_requires_checkout")
    );

    let no_stage = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), task.clone()]),
    )]);
    assert!(
        factor_obligation_steps(&no_stage, CHECKOUT, VERSION)
            .expect_err("the staged helper is mandatory")
            .to_string()
            .contains("declared_task_requires_staged_helper")
    );

    let mut credentialed = task;
    let StepKind::TaskExecution { env, .. } = &mut credentialed.kind else {
        unreachable!();
    };
    env.insert(
        "GITHUB_TOKEN".to_owned(),
        "${{ secrets.GITHUB_TOKEN }}".to_owned(),
    );
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), acquire_step(), credentialed]),
    )]);
    assert!(
        factor_obligation_steps(&jobs, CHECKOUT, VERSION)
            .expect_err("credentials cannot enter task action inputs")
            .to_string()
            .contains("credential_step_env:GITHUB_TOKEN")
    );
}

fn task_document_for_test() -> Yaml {
    let task = task_step(0);
    let StepKind::TaskExecution {
        argv,
        env,
        report_helper_version,
        ..
    } = &task.kind
    else {
        unreachable!();
    };
    let shape = super::Shape {
        argv_count: argv.len(),
        env_keys: env.keys().cloned().collect(),
        helper_version: report_helper_version.clone(),
    };
    super::declared_task_document(0, &shape).expect("typed composite document")
}

fn action_run_scalar(action: &Yaml) -> &str {
    let Yaml::Map(action_fields) = action else {
        panic!("composite action is a mapping");
    };
    let Some((_, Yaml::Map(runs_fields))) = action_fields.iter().find(|(key, _)| key == "runs")
    else {
        panic!("composite action has a runs mapping");
    };
    let Some((_, Yaml::Seq(steps))) = runs_fields.iter().find(|(key, _)| key == "steps") else {
        panic!("composite runs has a steps sequence");
    };
    let Some(Yaml::Map(step_fields)) = steps.first() else {
        panic!("composite action has one typed run step");
    };
    let Some((_, Yaml::Str(run))) = step_fields.iter().find(|(key, _)| key == "run") else {
        panic!("composite step has a run scalar");
    };
    run
}

fn checkout_step() -> Step {
    crate::steps::checkout_step(CHECKOUT).expect("configured checkout")
}

fn acquire_step() -> Step {
    let helper = format!("{}{}", crate::steps::STAGED_BINARY_PREFIX, VERSION);
    crate::steps::acquire_velnor_step(
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl -fsSL \"$VELNOR_ASSET_URL\" -o {helper} && echo \"$VELNOR_ASSET_SHA256  {helper}\" | sha256sum -c - && chmod +x {helper}"
            ),
        ],
        &BTreeMap::from([
            (
                crate::steps::ASSET_URL_ENV.to_owned(),
                "https://example.invalid/velnor".to_owned(),
            ),
            (crate::steps::ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
            (crate::steps::RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
        ]),
    )
    .expect("digest-verified helper staging")
}

fn task_step(index: usize) -> Step {
    let task_id = format!("stack/rust/crate-{index}/test/default");
    let toolchain_inputs = ToolchainInputs {
        tools: vec!["rust@1.99.0".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        "rust@1.99.0".to_owned(),
        "--".to_owned(),
        "cargo".to_owned(),
        "test".to_owned(),
        "-p".to_owned(),
        format!("crate-{index}"),
    ];
    let toolchain = toolchain_id(&toolchain_inputs).expect("toolchain id");
    let task_digest = task_digest_for_execution(&task_id, &argv, &toolchain).expect("task digest");
    let matrix_id =
        velnor_actions_contract::matrix_id_for_task_group("rust", &task_id).expect("matrix id");
    let matrix_key = velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let env = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.99.0".to_owned()),
    ]);
    let condition = velnor_actions_contract::workflow::step::task_execution_condition(&task_id)
        .expect("coverage condition");
    Step {
        name: format!("Test crate {index}"),
        id: None,
        role: None,
        condition: Some(condition),
        kind: StepKind::TaskExecution {
            argv,
            env,
            task_id,
            task_digest,
            toolchain_inputs,
            matrix_id,
            matrix_key,
            report_helper_version: VERSION.to_owned(),
            matrix_max_parallel: None,
        },
    }
}

fn simple_job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Rust / demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::new(20).expect("valid timeout"),
        needs: vec!["plan".to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps,
    }
}
