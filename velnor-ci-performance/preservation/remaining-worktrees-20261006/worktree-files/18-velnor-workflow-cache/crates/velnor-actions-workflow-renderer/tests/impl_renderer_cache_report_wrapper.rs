//! Exact generator report-wrapper tool inference regressions.
use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, matrix_key_for_id};
use velnor_actions_workflow_renderer::{
    cache_p08::{ensure_setup_p08, infer_job_tools},
    join_argv_for_run, shell_step,
};

use crate::impl_renderer_fixtures::{job, mbx_tool_env, mise};

const TASK_ID: &str = "stack/rust/demo/clippy/default";
const TASK_DIGEST: &str = "b3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MATRIX_ID: &str = "stack:rust|task:stack/rust/demo/clippy/default";
const HELPER_PATH: &str = "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0";
const CREDENTIAL_PRELUDE: &str = "unset ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN GITHUB_TOKEN MISE_GITHUB_TOKEN GH_TOKEN GH_HOST GH_CONFIG_DIR; ";

fn matrix_key() -> String {
    matrix_key_for_id(MATRIX_ID).expect("valid matrix id has a key")
}

fn start_path() -> String {
    format!("$RUNNER_TEMP/velnor/start-{}", matrix_key())
}

fn task_env() -> BTreeMap<String, String> {
    let mut env = mbx_tool_env("1.98.1");
    env.extend(BTreeMap::from([
        ("VELNOR_TASK_ID".to_owned(), TASK_ID.to_owned()),
        ("VELNOR_TASK_DIGEST".to_owned(), TASK_DIGEST.to_owned()),
        ("VELNOR_MATRIX_ID".to_owned(), MATRIX_ID.to_owned()),
        ("VELNOR_MATRIX_KEY".to_owned(), matrix_key()),
    ]));
    env
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
fn report_body(joined: &str) -> String {
    let start_path = start_path();
    format!(
        "date +%s%3N > \"{start_path}\"; {joined}; code=$?; read -r start_ms rest < \"{start_path}\"; VELNOR_EXIT_CODE=\"$code\" VELNOR_START_MS=\"$start_ms\" VELNOR_INTERNAL_OP=write-task-report-v1 \"{HELPER_PATH}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
    )
}

fn report_step(inner_argv: &[String]) -> Step {
    let joined = join_argv_for_run(inner_argv).expect("typed Mise argv joins");
    shell_step(
        "Clippy",
        vec!["sh".to_owned(), "-c".to_owned(), report_body(&joined)],
        task_env(),
    )
    .expect("captured report wrapper is a valid shell step")
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
        format!("{CREDENTIAL_PRELUDE}{}", report_body(&joined))
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
