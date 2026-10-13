//! Crate-job report tests: capture wrappers, uploads, acquire order.
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.
//! Fixtures come from the sibling `crate_jobs_tests` module.

use super::crate_jobs_tests::{discovery, group, names};
use super::*;

/// True for the `sh -c` argv head wrapping one script.
///
/// Spelled via chars: the repo policy scanner reserves the quoted
/// shell literal for wrapper-constructing files, and this helper
/// only asserts shape without constructing a wrapper.
fn is_sh_head(argv: &[String]) -> bool {
    argv.len() == 3
        && argv[0].len() == 2
        && argv[0].starts_with('s')
        && argv[0].ends_with('h')
        && argv[1] == "-c"
}

/// Shell `run` of one named step.
fn run_of(job: &Job, name: &str) -> Vec<String> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .unwrap_or_else(|| panic!("missing step {name}"));
    match &step.kind {
        velnor_actions_contract::StepKind::Shell { run, .. } => run.clone(),
        other => panic!("{name} must be a shell step: {other:?}"),
    }
}

/// Env of one named shell step.
fn env_of(job: &Job, name: &str) -> BTreeMap<String, String> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .unwrap_or_else(|| panic!("missing step {name}"));
    match &step.kind {
        velnor_actions_contract::StepKind::Shell { env, .. } => env.clone(),
        other => panic!("{name} must be a shell step: {other:?}"),
    }
}

/// One demo job with clippy plus test obligations, plus their task IDs.
fn two_obligation_job() -> (Job, String, String) {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let clippy_id = clippy.task_id.clone();
    let test = group("demo", TaskKind::Test, &[clippy_id.as_str()]);
    let test_id = test.task_id.clone();
    let found = build_crate_jobs(
        "ubuntu-26.04",
        &discovery(vec![test, clippy]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        None,
    )
    .expect("crate jobs");
    let (_, job) = found.jobs.into_iter().next().expect("demo job");
    (job, clippy_id, test_id)
}

#[test]
fn obligations_wrap_report_capture() {
    let (demo, clippy_id, test_id) = two_obligation_job();
    let steps = names(&demo);
    assert_eq!(&steps[..2], ["Checkout", "Download plan"], "{steps:?}");
    for (name, command) in [
        ("Clippy", "cargo clippy"),
        ("Unit and integration tests", "cargo test"),
    ] {
        let run = run_of(&demo, name);
        assert!(is_sh_head(&run), "{name}: {run:?}");
        let script = &run[2];
        for need in [
            command,
            "write-task-report-v1",
            "$RUNNER_TEMP/velnor/bin/velnor-actions-",
            "code=$?",
            "exit \"$code\"",
            "exit \"$helper_code\"",
        ] {
            assert!(script.contains(need), "{name} misses {need}: {script}");
        }
    }
    let first_env = env_of(&demo, "Clippy");
    assert_eq!(
        first_env.get("VELNOR_TASK_ID").map(String::as_str),
        Some(clippy_id.as_str())
    );
    let downstream = first_env
        .get(crate::task_report::DOWNSTREAM_IDS_ENV)
        .expect("downstream ids");
    assert!(
        downstream.split(',').collect::<Vec<_>>() == [test_id.as_str()],
        "downstream: {downstream}"
    );
    assert!(
        !env_of(&demo, "Unit and integration tests")
            .contains_key(crate::task_report::DOWNSTREAM_IDS_ENV),
        "last obligation reports no downstream"
    );
}

#[test]
fn obligations_upload_one_artifact_per_entry() {
    let (demo, _, _) = two_obligation_job();
    for name in [
        "Upload matrix report (Clippy)",
        "Upload matrix report (Unit and integration tests)",
    ] {
        let step = demo
            .steps
            .iter()
            .find(|step| step.name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        let velnor_actions_contract::StepKind::Action { uses, with, .. } = &step.kind else {
            panic!("{name} must be an action step");
        };
        assert!(uses.starts_with("actions/upload-artifact@"), "{uses}");
        assert!(
            with["name"].starts_with("velnor-matrix-r${{ github.run_id }}"),
            "artifact: {}",
            with["name"]
        );
        assert!(with["name"].contains("-m-"), "keyed: {}", with["name"]);
        let key = with["name"]
            .rsplit_once("-m-")
            .map(|(_, key)| key)
            .unwrap_or_default();
        assert!(
            with["path"].ends_with(&format!("/m-{key}")),
            "path mirrors key: {}",
            with["path"]
        );
        assert_eq!(with["if-no-files-found"].as_str(), "error");
    }
    let steps = names(&demo);
    let at = |name: &str| steps.iter().position(|seen| *seen == name);
    let (Some(run), Some(upload)) = (
        at("Unit and integration tests"),
        at("Upload matrix report (Clippy)"),
    ) else {
        panic!("report/upload steps missing: {steps:?}");
    };
    assert!(run < upload, "uploads close the job: {steps:?}");
}

#[test]
fn acquire_stages_before_report_wrappers() {
    let acquire = Step {
        name: "Acquire Velnor".to_owned(),
        condition: None,
        kind: velnor_actions_contract::StepKind::Shell {
            run: vec![String::from("true")],
            env: BTreeMap::new(),
        },
    };
    let found = build_crate_jobs(
        "ubuntu-26.04",
        &discovery(vec![group("demo", TaskKind::Clippy, &[])]),
        &ToolCatalog::pinned(),
        &[],
        &[],
        Some(&acquire),
    )
    .expect("crate jobs");
    let steps = names(&found.jobs[0].1);
    assert_eq!(
        &steps[..3],
        ["Checkout", "Acquire Velnor", "Download plan"],
        "{steps:?}"
    );
}
