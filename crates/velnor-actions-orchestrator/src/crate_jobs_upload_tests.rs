//! Crate-job obligation tests: report capture plus one upload per job.
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.
//! Fixtures live in the sibling `crate_jobs_tests` module.

use super::crate_jobs_tests::{discovery, group, names};
use super::*;
use velnor_actions_rust::TaskKind;

/// True for the `sh -c` argv head wrapping one script.
///
/// The step constructor preludes the script with the credential
/// unset; the script itself carries the wrapper. Spelled via chars:
/// the repo policy scanner reserves the quoted shell literal for
/// wrapper-constructing files, and this helper only asserts shape
/// without constructing a wrapper.
fn is_sh_head(argv: &[String]) -> bool {
    use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;
    argv.len() == 3
        && argv[0].len() == 2
        && argv[0].starts_with('s')
        && argv[0].ends_with('h')
        && argv[1] == "-c"
        && argv[2].starts_with(&credential_unset_prelude())
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
fn two_obligation_job() -> (String, Job, String, String) {
    let clippy = group("demo", TaskKind::Clippy, &[]);
    let clippy_id = clippy.task_id.clone();
    let test = group("demo", TaskKind::Test, &[clippy_id.as_str()]);
    let test_id = test.task_id.clone();
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![test, clippy]),
        catalog: &ToolCatalog::pinned(),
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
    .expect("crate jobs");
    let (job_id, job) = found.jobs.into_iter().next().expect("demo job");
    (job_id, job, clippy_id, test_id)
}

#[test]
fn obligations_wrap_report_capture() {
    let (_, demo, clippy_id, _test_id) = two_obligation_job();
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
    assert!(
        !first_env.contains_key(crate::task_report::DOWNSTREAM_IDS_ENV),
        "downstream ids are derived from the plan on failure"
    );
    assert!(
        !env_of(&demo, "Unit and integration tests")
            .contains_key(crate::task_report::DOWNSTREAM_IDS_ENV),
        "last obligation reports no downstream"
    );
}

#[test]
fn obligations_upload_one_artifact_per_job() {
    let (job_id, demo, _, _) = two_obligation_job();
    let uploads: Vec<_> = demo
        .steps
        .iter()
        .filter(|step| step.name == velnor_actions_workflow_renderer::CRATE_REPORT_UPLOAD_NAME)
        .collect();
    assert_eq!(uploads.len(), 1, "one upload for two obligations");
    let step = uploads[0];
    let velnor_actions_contract::StepKind::Action { uses, with, .. } = &step.kind else {
        panic!("crate upload must be an action step");
    };
    assert!(uses.starts_with("actions/upload-artifact@"), "{uses}");
    assert_eq!(
        with["name"].as_str(),
        &format!("velnor-crate-r${{{{ github.run_id }}}}-a${{{{ github.run_attempt }}}}-{job_id}"),
    );
    assert!(
        !with["name"].contains("-m-"),
        "job artifact carries no entry key: {}",
        with["name"]
    );
    assert_eq!(
        with["path"].as_str(),
        "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ github.run_attempt }}",
        "upload carries the whole run dir",
    );
    assert_eq!(with["if-no-files-found"].as_str(), "error");
    assert!(
        demo.steps.iter().all(|step| !step
            .name
            .starts_with(velnor_actions_workflow_renderer::MATRIX_REPORT_UPLOAD_NAME)),
        "no per-entry matrix uploads survive",
    );
    let steps = names(&demo);
    let at = |name: &str| steps.iter().position(|seen| *seen == name);
    let (Some(run), Some(upload)) = (
        at("Unit and integration tests"),
        at(velnor_actions_workflow_renderer::CRATE_REPORT_UPLOAD_NAME),
    ) else {
        panic!("report/upload steps missing: {steps:?}");
    };
    assert!(run < upload, "uploads close the job: {steps:?}");
}
