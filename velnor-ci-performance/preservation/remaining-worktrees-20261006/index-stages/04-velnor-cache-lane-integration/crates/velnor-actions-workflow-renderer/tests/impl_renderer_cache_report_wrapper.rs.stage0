//! Exact generator report-wrapper tool inference regressions.
use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, matrix_id_for_task_group, matrix_key_for_id};
use velnor_actions_workflow_renderer::{
    cache_p08::{ensure_setup_p08, infer_job_tools},
    join_argv_for_run, shell_step,
};

use crate::impl_renderer_fixtures::{job, mbx_tool_env, mise};

const TASK_ID: &str = "stack/rust/demo/clippy/default";
const TASK_DIGEST: &str = "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MATRIX_ID: &str = "stack:rust|task:stack/rust/demo/clippy/default";
const BUILD_TASK_ID: &str = "stack/rust/demo/build/default";
const NEXTEST_TASK_ID: &str = "stack/rust/demo/nextest/default";
const TEST_MBX_VERSION: &str = "1.21.1";
const TEST_NEXTTEST_VERSION: &str = "0.9.146";
const TEST_RUST_TOOLCHAIN: &str = "1.98.1";
const HELPER_PATH: &str = concat!(
    "$RUNNER_TEMP/velnor/bin/velnor-actions-",
    env!("CARGO_PKG_VERSION")
);
const CREDENTIAL_PRELUDE: &str = "unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR; ";

#[path = "impl_renderer_cache_report_preparation.rs"]
mod preparation_tests;

fn matrix_key() -> String {
    matrix_key_for_id(MATRIX_ID).expect("valid matrix id has a key")
}

fn task_matrix_id(task_id: &str) -> String {
    matrix_id_for_task_group("rust", task_id).expect("valid task id has a matrix id")
}

fn task_matrix_key(task_id: &str) -> String {
    matrix_key_for_id(&task_matrix_id(task_id)).expect("valid matrix id has a key")
}

fn start_path(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/start-{matrix_key}")
}

fn task_env_for(task_id: &str) -> BTreeMap<String, String> {
    let matrix_id = task_matrix_id(task_id);
    let matrix_key = matrix_key_for_id(&matrix_id).expect("valid matrix id has a key");
    let mut env = mbx_tool_env(TEST_RUST_TOOLCHAIN);
    env.extend(BTreeMap::from([
        ("VELNOR_TASK_ID".to_owned(), task_id.to_owned()),
        ("VELNOR_TASK_DIGEST".to_owned(), TASK_DIGEST.to_owned()),
        ("VELNOR_MATRIX_ID".to_owned(), matrix_id),
        ("VELNOR_MATRIX_KEY".to_owned(), matrix_key),
    ]));
    env
}

fn task_env() -> BTreeMap<String, String> {
    task_env_for(TASK_ID)
}

fn clippy_argv() -> Vec<String> {
    [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.98.1",
        "--",
        "cargo",
        "clippy",
        "--locked",
        "--offline",
        "--manifest-path",
        "Cargo.toml",
        "--package",
        "demo",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Captured shape emitted by matrix_step::report_wrapper_argv; shell_step
/// contributes the exact credential-unset prelude around this body.
fn report_body(joined: &str, matrix_key: &str) -> String {
    let start_path = start_path(matrix_key);
    format!(
        "date +%s%3N > \"{start_path}\"; {joined}; code=$?; read -r start_ms rest < \"{start_path}\"; VELNOR_EXIT_CODE=\"$code\" VELNOR_START_MS=\"$start_ms\" VELNOR_INTERNAL_OP=write-task-report-v1 \"{HELPER_PATH}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
    )
}

fn report_step(inner_argv: &[String]) -> Step {
    report_step_for(inner_argv, TASK_ID, "Clippy")
}

fn report_step_for(inner_argv: &[String], task_id: &str, name: &str) -> Step {
    let joined = join_argv_for_run(inner_argv).expect("typed Mise argv joins");
    let matrix_key = task_matrix_key(task_id);
    let mut step = shell_step(
        name,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            report_body(&joined, &matrix_key),
        ],
        task_env_for(task_id),
    )
    .expect("captured report wrapper is a valid shell step");
    step.condition = Some(format!(
        "!contains(needs.plan.outputs.{}, ',{task_id},')",
        velnor_actions_workflow_renderer::COVERED_TASKS_OUTPUT
    ));
    step
}

fn script_mut(step: &mut Step) -> &mut String {
    let StepKind::Shell { run, .. } = &mut step.kind else {
        panic!("report wrapper must be a shell step");
    };
    &mut run[2]
}

fn assert_setup_rejects(step: Step, id: &str) {
    let (job_id, mut workflow_job) = job(id, "Report wrapper", Vec::new(), vec![step]);
    assert!(
        ensure_setup_p08(
            &job_id,
            &mut workflow_job,
            &mise(),
            false,
            "x86_64-unknown-linux-gnu"
        )
        .is_err(),
        "noncanonical report wrappers fail closed"
    );
}

#[test]
fn exact_report_wrapper_extracts_only_the_typed_clippy_tool() {
    let argv = clippy_argv();
    let joined = join_argv_for_run(&argv).expect("typed Mise argv joins");
    let matrix_key = matrix_key();
    let step = report_step(&argv);
    let StepKind::Shell { run, env } = &step.kind else {
        panic!("report wrapper must be a shell step");
    };
    assert_eq!(&run[..2], ["sh".to_owned(), "-c".to_owned()]);
    assert_eq!(
        run[2],
        format!("{CREDENTIAL_PRELUDE}{}", report_body(&joined, &matrix_key))
    );
    assert_eq!(task_matrix_id(TASK_ID), MATRIX_ID);
    assert_eq!(
        step.condition.as_deref(),
        Some("!contains(needs.plan.outputs.covered_tasks, ',stack/rust/demo/clippy/default,')")
    );
    for (key, value) in [
        ("VELNOR_TASK_ID", TASK_ID),
        ("VELNOR_TASK_DIGEST", TASK_DIGEST),
        ("VELNOR_MATRIX_ID", MATRIX_ID),
        ("VELNOR_MATRIX_KEY", matrix_key.as_str()),
        (
            "MISE_DATA_DIR",
            "${{ github.workspace }}/.velnor/cache/mise",
        ),
        (
            "MISE_RUSTUP_HOME",
            "${{ github.workspace }}/.velnor/cache/rustup",
        ),
        (
            "MISE_CARGO_HOME",
            "${{ github.workspace }}/.velnor/cache/cargo",
        ),
        (
            "RUSTUP_HOME",
            "${{ github.workspace }}/.velnor/cache/rustup",
        ),
        ("CARGO_HOME", "${{ github.workspace }}/.velnor/cache/cargo"),
        ("RUSTUP_TOOLCHAIN", "1.98.1"),
    ] {
        assert_eq!(env.get(key).map(String::as_str), Some(value), "{key}");
    }
    let (_, mut workflow_job) = job("report-clippy", "Report Clippy", Vec::new(), vec![step]);
    assert_eq!(infer_job_tools(&workflow_job), ["rust@1.98.1".to_owned()]);
    assert!(
        ensure_setup_p08(
            "report-clippy",
            &mut workflow_job,
            &mise(),
            false,
            "x86_64-unknown-linux-gnu"
        )
        .is_ok()
    );
}

#[test]
fn report_wrapper_mutations_and_arbitrary_shells_fail_closed() {
    let mut missing_condition = report_step(&clippy_argv());
    missing_condition.condition = None;
    assert_setup_rejects(missing_condition, "report-condition-missing");

    let mut wrong_condition = report_step(&clippy_argv());
    wrong_condition.condition = Some("always()".to_owned());
    assert_setup_rejects(wrong_condition, "report-condition-mutated");

    let mut body = report_step(&clippy_argv());
    let script = script_mut(&mut body);
    let changed = script.replace(
        "; helper_code=$?; if [ \"$code\" -ne 0 ]; then",
        "; helper_code=$?; if [ \"$helper_code\" -ne 0 ]; then",
    );
    assert_ne!(*script, changed, "mutation changes the report body");
    *script = changed;
    assert_setup_rejects(body, "report-body-mutated");

    let mut prefix = report_step(&clippy_argv());
    let script = script_mut(&mut prefix);
    let changed = script.replace(" GH_HOST GH_CONFIG_DIR; date", " GH_CONFIG_DIR; date");
    assert_ne!(*script, changed, "mutation changes the credential prelude");
    *script = changed;
    assert_setup_rejects(prefix, "report-credential-mutated");

    let mut missing_hooks = clippy_argv();
    missing_hooks.retain(|arg| arg != "--no-hooks");
    assert_setup_rejects(report_step(&missing_hooks), "report-inner-flags-mutated");

    let mut wrong_child = clippy_argv();
    let cargo = wrong_child
        .iter()
        .position(|arg| arg == "cargo")
        .expect("cargo child");
    wrong_child.truncate(cargo);
    wrong_child.extend(["cargo", "fmt", "--check"].map(str::to_owned));
    assert_setup_rejects(report_step(&wrong_child), "report-inner-task-mutated");

    let mut moving_pin = clippy_argv();
    let rust = moving_pin
        .iter_mut()
        .find(|arg| arg.as_str() == "rust@1.98.1")
        .expect("Rust pin");
    *rust = "rust@latest".to_owned();
    assert_setup_rejects(report_step(&moving_pin), "report-inner-pin-mutated");

    let joined = join_argv_for_run(&clippy_argv()).expect("typed Mise argv joins");
    let arbitrary = shell_step(
        "Arbitrary shell",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!("{joined}; if true; then echo arbitrary; fi"),
        ],
        task_env(),
    )
    .expect("shell step is structurally valid");
    assert_setup_rejects(arbitrary, "arbitrary-sh-if");
}
