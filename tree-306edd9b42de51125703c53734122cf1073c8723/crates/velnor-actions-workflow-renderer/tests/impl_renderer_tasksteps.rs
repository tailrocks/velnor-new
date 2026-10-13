//! Matrix named steps: full build, no-op reports, doc/doctest, timings.
use std::collections::BTreeMap;
use velnor_actions_contract::NotSelectedReason;
use velnor_actions_workflow_renderer::task_steps::{
    CLIPPY_NAME, DOCTESTS_NAME, DOCUMENTATION_NAME, LEG_EVENT_ENV, LEG_MATRIX_ID_ENV,
    LEG_MATRIX_KEY_ENV, LEG_TASK_DIGEST_ENV, LEG_TASK_ID_ENV, LEG_TASK_RUN_ENV, NOOP_EVENT_ENV,
    NOOP_MATRIX_ID_ENV, NOOP_MATRIX_KEY_ENV, NoOpReport, REPORT_TIMINGS_NAME, RESTORE_OBJECTS_NAME,
    TASK_STEP_NAMES, TaskStepMode, TaskStepSpec, build_task_steps, check_doc_after_doctest,
    check_task_step_order, check_timings_report_last, leg_execution_script, noop_report_script,
    noop_step, timings_report_step,
};
use velnor_actions_workflow_renderer::{RenderError, checkout_step, shell_step};

use super::impl_renderer_fixtures::*;

fn noop_report() -> NoOpReport {
    NoOpReport {
        task_id: "stack/rust/crates/velnor-actions-contract/clippy/default".to_owned(),
        task_digest: format!("b3-{}", "a".repeat(64)),
        reason: NotSelectedReason::Unsupported,
    }
}

fn execute_specs() -> Vec<TaskStepSpec> {
    TASK_STEP_NAMES
        .iter()
        .map(|name| TaskStepSpec {
            name: (*name).to_owned(),
            mode: TaskStepMode::Execute {
                argv: mise_argv("rust@1.98.1", "cargo", &["clippy"]),
                env: BTreeMap::new(),
            },
        })
        .collect()
}

#[test]
fn task_steps_build_all_twelve_in_order() -> Result<(), RenderError> {
    let built = build_task_steps(&execute_specs())?;
    let names: Vec<&str> = built.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(names, TASK_STEP_NAMES);
    Ok(())
}

#[test]
fn task_steps_reject_misordered_or_missing() {
    let mut swapped = execute_specs();
    swapped.swap(8, 9);
    assert!(
        build_task_steps(&swapped)
            .is_err_and(|err| format!("{err:?}").contains("task_steps_misordered")),
        "swapped doctest/doc must fail"
    );
    let short = &execute_specs()[..11];
    assert!(
        build_task_steps(short)
            .is_err_and(|err| format!("{err:?}").contains("task_steps_wrong_count")),
        "eleven specs must fail"
    );
}

#[test]
fn task_noop_step_writes_explanatory_report() -> Result<(), RenderError> {
    let step = noop_step(RESTORE_OBJECTS_NAME, &noop_report())?;
    assert_eq!(step.name, RESTORE_OBJECTS_NAME);
    let velnor_actions_contract::StepKind::Shell { run, env } = &step.kind else {
        panic!("no-op must be a shell step");
    };
    assert_eq!(
        env[NOOP_MATRIX_KEY_ENV].as_str(),
        "${{ matrix.matrix_key }}"
    );
    assert_eq!(env[NOOP_MATRIX_ID_ENV].as_str(), "${{ matrix.id }}");
    assert_eq!(env[NOOP_EVENT_ENV].as_str(), "${{ github.event_name }}");
    let script = run[2].as_str();
    for token in [
        "stack/rust/crates/velnor-actions-contract/clippy/default",
        "not_selected",
        "not_selected_reason",
        "unsupported",
        "schema",
        "/tasks/",
        "task_report_id",
        "not_attempted",
    ] {
        assert!(script.contains(token), "missing {token}:\n{script}");
    }
    assert!(
        !script.contains('\''),
        "single quotes break quoting:\n{script}"
    );
    assert!(
        noop_step("Ad hoc", &noop_report())
            .is_err_and(|err| format!("{err:?}").contains("noop_bad_name")),
        "ad-hoc no-op name must fail"
    );
    let mut bad_id = noop_report();
    bad_id.task_id = "not-a-task-id".to_owned();
    assert!(
        matches!(noop_report_script(&bad_id), Err(RenderError::Contract(_))),
        "bad task id must fail"
    );
    let mut bad_digest = noop_report();
    bad_digest.task_digest = "b3-too-short".to_owned();
    assert!(
        matches!(
            noop_report_script(&bad_digest),
            Err(RenderError::Contract(_))
        ),
        "bad digest must fail"
    );
    Ok(())
}

#[test]
fn task_doc_must_follow_doctests() -> Result<(), RenderError> {
    let full = job(
        "velnor-task",
        "Velnor Task",
        Vec::new(),
        build_task_steps(&execute_specs())?,
    );
    check_task_step_order(&full.1)?;
    check_doc_after_doctest(&full.1)?;
    let argv = vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()];
    let flipped = job(
        "velnor-task",
        "Velnor Task",
        Vec::new(),
        vec![
            shell_step(DOCUMENTATION_NAME, argv.clone(), BTreeMap::new())?,
            shell_step(DOCTESTS_NAME, argv.clone(), BTreeMap::new())?,
        ],
    );
    assert!(
        check_doc_after_doctest(&flipped.1)
            .is_err_and(|err| format!("{err:?}").contains("doc_before_doctest")),
        "doc before doctest must fail"
    );
    let missing = job(
        "velnor-task",
        "Velnor Task",
        Vec::new(),
        vec![shell_step(CLIPPY_NAME, argv, BTreeMap::new())?],
    );
    assert!(check_doc_after_doctest(&missing.1).is_err());
    assert!(check_task_step_order(&missing.1).is_err());
    Ok(())
}

#[test]
fn task_steps_render_in_order_with_mixed_modes() -> Result<(), RenderError> {
    let mut specs = execute_specs();
    specs[3].mode = TaskStepMode::NoOp {
        report: noop_report(),
    };
    let mut steps = vec![checkout_step(&checkout_pin())?];
    steps.extend(build_task_steps(&specs)?);
    let text = strict(
        &fixture_ir(vec![job("velnor-task", "Velnor Task", Vec::new(), steps)]),
        &fixture_ctx(),
    )?;
    let named: Vec<String> = step_names(&text, "velnor-task")
        .into_iter()
        .filter(|name| TASK_STEP_NAMES.contains(&name.as_str()))
        .collect();
    assert_eq!(named, TASK_STEP_NAMES);
    assert!(
        text.contains("not_selected_reason"),
        "no-op report:\n{text}"
    );
    Ok(())
}

#[test]
fn task_timings_report_is_last_and_names_matrix_report() -> Result<(), RenderError> {
    let argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        "write \"$RUNNER_TEMP/velnor/x/matrix-report.json\"".to_owned(),
    ];
    let step = timings_report_step(argv, BTreeMap::new())?;
    assert_eq!(step.name, REPORT_TIMINGS_NAME);
    assert!(
        timings_report_step(
            vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
            BTreeMap::new(),
        )
        .is_err_and(|err| format!("{err:?}").contains("timings_without_matrix_report")),
        "timings without matrix report must fail"
    );
    let full = job(
        "velnor-task",
        "Velnor Task",
        Vec::new(),
        build_task_steps(&execute_specs())?,
    );
    check_timings_report_last(&full.1)?;
    let mut late = full.1.clone();
    late.steps.push(shell_step(
        CLIPPY_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
        BTreeMap::new(),
    )?);
    assert!(
        check_timings_report_last(&late)
            .is_err_and(|err| format!("{err:?}").contains("timings_report_not_last")),
        "named step after timings must fail"
    );
    Ok(())
}

#[test]
fn leg_script_runs_command_and_always_reports() -> Result<(), RenderError> {
    let script = leg_execution_script();
    for want in [
        ":?matrix.run_missing",
        ":?matrix.task_digest_missing",
        "code=$?",
        "matrix-report.json",
        "/tasks/",
        "cut -c4-19",
        "exit $code",
    ] {
        assert!(script.contains(want), "missing {want}:\n{script}");
    }
    for banned in ["$(", "`", "${{", "cargo", "mbx", "nextest"] {
        assert!(!script.contains(banned), "banned {banned}:\n{script}");
    }
    let env = BTreeMap::from([
        (
            LEG_TASK_ID_ENV.to_owned(),
            "${{ matrix.task_id }}".to_owned(),
        ),
        (LEG_TASK_RUN_ENV.to_owned(), "${{ matrix.run }}".to_owned()),
        (
            LEG_TASK_DIGEST_ENV.to_owned(),
            "${{ matrix.task_digest }}".to_owned(),
        ),
        (
            LEG_MATRIX_KEY_ENV.to_owned(),
            "${{ matrix.matrix_key }}".to_owned(),
        ),
        (LEG_MATRIX_ID_ENV.to_owned(), "${{ matrix.id }}".to_owned()),
        (
            LEG_EVENT_ENV.to_owned(),
            "${{ github.event_name }}".to_owned(),
        ),
    ]);
    shell_step(
        "Run task",
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    Ok(())
}

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const LEG_DIGEST: &str = "b3-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const LEG_MATRIX_KEY: &str = "m-0123456789abcdef";
const LEG_TASK_ID: &str = "stack/rust/crates/demo/build/default";

fn leg_stub_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("velnor-leg-{}-{tag}", std::process::id()))
}

fn run_leg_script(run: &str, temp: &std::path::Path) -> TestResult<std::process::Output> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(leg_execution_script())
        .env(LEG_TASK_RUN_ENV, run)
        .env(LEG_TASK_ID_ENV, LEG_TASK_ID)
        .env(LEG_TASK_DIGEST_ENV, LEG_DIGEST)
        .env(LEG_MATRIX_KEY_ENV, LEG_MATRIX_KEY)
        .env(LEG_MATRIX_ID_ENV, format!("stack:rust|task:{LEG_TASK_ID}"))
        .env(LEG_EVENT_ENV, "pull_request")
        .env("GITHUB_RUN_ID", "7")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("RUNNER_TEMP", temp)
        .output()?;
    Ok(output)
}

fn read_json(path: &std::path::Path) -> TestResult<serde_json::Value> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn leg_dir(temp: &std::path::Path) -> std::path::PathBuf {
    temp.join("velnor/r7-a1").join(LEG_MATRIX_KEY)
}

#[test]
fn leg_script_reports_parse_as_json_on_success() -> TestResult {
    let temp = leg_stub_dir("pass");
    let output = run_leg_script("true", &temp)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "leg failed: {stderr}");
    let dir = leg_dir(&temp);
    let matrix = read_json(&dir.join("matrix-report.json"))?;
    assert_eq!(matrix["schema"], 1);
    assert_eq!(matrix["status"], "passed");
    assert_eq!(matrix["executed"], 1);
    assert_eq!(matrix["failed"], 0);
    let task_report_id = matrix["task_report_ids"][0]
        .as_str()
        .expect("task_report_ids[0] is a string");
    let task = read_json(&dir.join("tasks").join(format!("{task_report_id}.json")))?;
    assert_eq!(task["schema"], 1);
    assert_eq!(task["status"], "executed");
    assert_eq!(task["exit_code"], 0);
    assert_eq!(task["task_id"], LEG_TASK_ID);
    assert_eq!(task["task_digest"], LEG_DIGEST);
    std::fs::remove_dir_all(&temp)?;
    Ok(())
}

#[test]
fn leg_script_reports_parse_as_json_on_failure() -> TestResult {
    let temp = leg_stub_dir("fail");
    let output = run_leg_script("false", &temp)?;
    assert_eq!(output.status.code(), Some(1));
    let dir = leg_dir(&temp);
    let matrix = read_json(&dir.join("matrix-report.json"))?;
    assert_eq!(matrix["status"], "failed");
    assert_eq!(matrix["executed"], 0);
    assert_eq!(matrix["failed"], 1);
    let task_report_id = matrix["task_report_ids"][0]
        .as_str()
        .expect("task_report_ids[0] is a string");
    let task = read_json(&dir.join("tasks").join(format!("{task_report_id}.json")))?;
    assert_eq!(task["status"], "failed");
    assert_eq!(task["exit_code"], 1);
    std::fs::remove_dir_all(&temp)?;
    Ok(())
}
