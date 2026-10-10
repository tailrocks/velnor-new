use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};

use super::{ACTION_NAME_PREFIX, factor_obligation_steps};
use crate::toolchain_env;

const CHECKOUT: &str = "actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HELPER: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.6";
const VERSION: &str = "0.1.6";

#[test]
fn one_typed_action_serves_all_150_obligation_jobs_without_dropping_headers() {
    let jobs = (0..150)
        .map(|index| {
            let id = format!("rust-crate-{index}");
            let condition = if index % 2 == 0 {
                "success()"
            } else {
                "always()"
            };
            let job = Job {
                display_name: format!("Rust / crate {index}"),
                runs_on: "ubuntu-26.04".to_owned(),
                check_runner: None,
                timeout_minutes: JobTimeout::new(20).expect("valid timeout"),
                needs: vec!["plan".to_owned()],
                condition: Some("needs.plan.result == 'success'".to_owned()),
                permissions: None,
                environment: None,
                steps: vec![checkout_step(), obligation_step(index, condition)],
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
    assert!(files[0].bytes.contains("argv=( \"$VELNOR_WRAPPER_ARGV_0\""));
    assert!(files[0].bytes.contains("write-task-report-v1"));
    assert!(
        files[0]
            .bytes
            .contains("if [ \"$task_code\" -ne 0 ]; then exit \"$task_code\"; fi")
    );
    assert!(
        files[0]
            .bytes
            .contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN")
    );
    assert!(!files[0].bytes.contains("eval"));
    for key in toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(toolchain_env::STEP_ENDPOINT_DENYLIST)
    {
        assert!(!files[0].bytes.contains(&format!("inputs.env_{key}")));
    }

    for (index, (id, original)) in jobs.iter().enumerate() {
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
        let caller = &rewritten.steps[1];
        assert_eq!(caller.name, original.steps[1].name);
        assert_eq!(caller.condition, original.steps[1].condition);
        let StepKind::Action { uses, with, env } = &caller.kind else {
            panic!("obligation was not replaced by its composite call");
        };
        assert_eq!(uses, "./.github/actions/declared-task-0");
        assert!(env.is_empty(), "task env crosses through declared inputs");
        assert_eq!(with["argv_5"], format!("crate-{index}"));
        assert_eq!(with["env_VELNOR_TASK_ID"], format!("task/{index}"));
        assert!(!with.contains_key("env_GITHUB_TOKEN"));
    }
}

#[test]
fn dynamic_shell_expansion_and_typed_tofu_role_stay_unfactored() {
    let mut dynamic = obligation_step(0, "success()");
    let StepKind::Shell { run, .. } = &mut dynamic.kind else {
        panic!("fixture shell step");
    };
    let command = crate::commands::join_argv_for_run(&[
        "mise".to_owned(),
        "exec".to_owned(),
        "--tool=$HOME".to_owned(),
    ])
    .expect("valid dynamic shell fixture");
    run[run.len() - 1] = format!(
        "{}s=$(date +%s%3N); {command}; code=$?; VELNOR_EXIT_CODE=\"$code\" VELNOR_START_MS=\"$s\" VELNOR_INTERNAL_OP=write-task-report-v1 \"{HELPER}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\"",
        toolchain_env::credential_unset_prelude()
    );
    let mut tofu = obligation_step(1, "success()");
    tofu.role = Some(StepRole::TofuProviderUse);
    let jobs = BTreeMap::from([(
        "rust-demo".to_owned(),
        simple_job(vec![checkout_step(), dynamic.clone(), tofu.clone()]),
    )]);

    let (factored, files) =
        factor_obligation_steps(&jobs, CHECKOUT, VERSION).expect("fail-closed factor");
    assert_eq!(files.len(), 0);
    assert_eq!(
        factored.get("rust-demo").expect("retained job").steps,
        jobs.get("rust-demo").expect("original job").steps
    );
}

fn checkout_step() -> Step {
    Step {
        name: "Checkout".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Action {
            uses: CHECKOUT.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn obligation_step(index: usize, condition: &str) -> Step {
    let mut env = BTreeMap::from([
        ("VELNOR_TASK_ID".to_owned(), format!("task/{index}")),
        ("VELNOR_TASK_DIGEST".to_owned(), format!("digest-{index}")),
        ("VELNOR_MATRIX_ID".to_owned(), format!("matrix-{index}")),
        ("VELNOR_MATRIX_KEY".to_owned(), format!("key-{index}")),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
    ]);
    env.extend(toolchain_env::credential_scrub());
    let argv = vec![
        "mise".to_owned(),
        "exec".to_owned(),
        "--tool=cargo".to_owned(),
        "test".to_owned(),
        "-p".to_owned(),
        format!("crate-{index}"),
    ];
    let joined = crate::commands::join_argv_for_run(&argv).expect("fixed task argv");
    let run = format!(
        "s=$(date +%s%3N); {joined}; code=$?; VELNOR_EXIT_CODE=\"$code\" VELNOR_START_MS=\"$s\" VELNOR_INTERNAL_OP=write-task-report-v1 \"{HELPER}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
    );
    let mut step = crate::steps::shell_step(
        &format!("Run crate {index}"),
        vec!["sh".to_owned(), "-c".to_owned(), run],
        env,
    )
    .expect("validated fixture step");
    step.condition = Some(condition.to_owned());
    step
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
