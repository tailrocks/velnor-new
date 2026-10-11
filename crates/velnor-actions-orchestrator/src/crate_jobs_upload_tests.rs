//! Crate-job obligation tests: report capture plus one upload per job.
//!
//! Declared via `#[path]` from `crate_jobs.rs` under `cfg(test)`.
//! Fixtures live in the sibling `crate_jobs_tests` module.

use super::crate_jobs_tests::{discovery, group, names};
use super::*;
use velnor_actions_rust::{CompileDriver, TaskKind};

/// One demo job with clippy plus test obligations on one selected driver.
fn two_obligation_job(driver: CompileDriver) -> (String, Job, ProposedTask, ProposedTask) {
    let mut clippy = group("demo", TaskKind::Clippy, &[]);
    clippy.identity.compile_driver = driver.as_str().to_owned();
    clippy.validate().expect("clippy proposal");
    let clippy_id = clippy.task_id.clone();
    let mut test = group("demo", TaskKind::Test, &[clippy_id.as_str()]);
    test.identity.compile_driver = driver.as_str().to_owned();
    test.validate().expect("test proposal");
    let catalog = ToolCatalog::pinned();
    let found = build_crate_jobs(CrateJobInputs {
        label: "ubuntu-26.04",
        policy: WorkflowPolicy::ConsumerV1,
        discovery: &discovery(vec![test.clone(), clippy.clone()]),
        catalog: &catalog,
        fetch_roots: &[],
        acquire: None,
        max_parallel_jobs: 2,
        helper_version: env!("CARGO_PKG_VERSION"),
    })
    .expect("crate jobs");
    let (job_id, job) = found.jobs.into_iter().next().expect("demo job");
    (job_id, job, clippy, test)
}

/// Compare the emitted command and exact catalog-backed toolchain.
fn assert_task_command(
    task: &ProposedTask,
    argv: &[String],
    toolchain_inputs: &velnor_actions_contract::cachekey::ToolchainInputs,
    task_digest: &str,
    catalog: &ToolCatalog,
) {
    let expected_argv = crate::vectors::task_argv(task, catalog).expect("task argv");
    assert_eq!(
        argv,
        expected_argv.as_slice(),
        "{} pinned command",
        task.task_id
    );
    let separator = argv
        .iter()
        .position(|arg| arg == "--")
        .expect("Mise exec separator");
    assert_eq!(argv.get(separator + 1).map(String::as_str), Some("mbx"));

    let expected_toolchain = crate::internal_plan::identities::toolchain_inputs_for(task, catalog)
        .expect("pinned toolchain inputs");
    assert_eq!(toolchain_inputs, &expected_toolchain, "toolchain inputs");
    assert_eq!(
        expected_toolchain.compile_driver,
        CompileDriver::Mbx.as_str()
    );
    assert!(
        expected_toolchain
            .tools
            .contains(&catalog.tool_spec(PinnedTool::Rust))
    );
    assert!(
        expected_toolchain
            .tools
            .contains(&catalog.tool_spec(PinnedTool::MrBoxington))
    );
    let toolchain_id = velnor_actions_contract::cachekey::toolchain_id(&expected_toolchain)
        .expect("toolchain identity");
    let expected_digest = velnor_actions_contract::workflow::crate_job::task_digest_for_execution(
        &task.task_id,
        &expected_argv,
        &toolchain_id,
    )
    .expect("plan task digest");
    assert_eq!(task_digest, expected_digest.as_str(), "plan-bound digest");
}

/// Check the typed identity and report-helper authority for one obligation.
fn assert_task_report_identity(
    task: &ProposedTask,
    step: &Step,
    task_id: &str,
    matrix_id: &str,
    matrix_key: &str,
    helper_version: &str,
    matrix_max_parallel: Option<u32>,
) {
    assert_eq!(task_id, task.task_id.as_str(), "report lookup identity");
    let expected_matrix_id =
        velnor_actions_contract::matrix_id_for_task_group("rust", &task.task_id)
            .expect("matrix identity");
    let expected_matrix_key =
        velnor_actions_contract::matrix_key_for_id(&expected_matrix_id).expect("matrix key");
    assert_eq!(matrix_id, expected_matrix_id.as_str(), "matrix identity");
    assert_eq!(matrix_key, expected_matrix_key.as_str(), "matrix key");
    assert_eq!(matrix_max_parallel, None, "crate obligations are uncapped");
    assert_eq!(helper_version, env!("CARGO_PKG_VERSION"));
    let expected_condition =
        crate::covered_tasks::skip_condition(&task.task_id).expect("task condition");
    assert_eq!(
        step.condition.as_deref(),
        Some(expected_condition.as_str()),
        "plan-coverage gate"
    );
}

/// Check that only task payload/isolation values enter the task environment.
fn assert_task_environment(
    task: &ProposedTask,
    env: &BTreeMap<String, String>,
    catalog: &ToolCatalog,
) {
    let payload_env = velnor_actions_rust::payload_env_for_kind(&task.task_kind)
        .into_iter()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    let expected_env = crate::matrix_step::task_step_env(catalog, &payload_env, true)
        .expect("validated task environment");
    assert_eq!(env, &expected_env, "exact task environment");
    assert!(
        !env.contains_key(crate::task_report::TASK_ID_ENV)
            && !env.contains_key(crate::task_report::DOWNSTREAM_IDS_ENV),
        "report authority stays in TaskExecution fields, not environment"
    );
}

/// Assert one generated step keeps report authority in its typed payload.
fn assert_task_execution_matches(task: &ProposedTask, step: &Step, catalog: &ToolCatalog) {
    let velnor_actions_contract::StepKind::TaskExecution {
        argv,
        env,
        task_id,
        task_digest,
        toolchain_inputs,
        matrix_id,
        matrix_key,
        report_helper_version,
        matrix_max_parallel,
    } = &step.kind
    else {
        panic!(
            "{} lost typed task/report authority: {:?}",
            task.task_id, step.kind
        );
    };
    assert_task_command(task, argv, toolchain_inputs, task_digest, catalog);
    assert_task_report_identity(
        task,
        step,
        task_id,
        matrix_id,
        matrix_key,
        report_helper_version,
        *matrix_max_parallel,
    );
    assert_task_environment(task, env, catalog);
}

#[test]
fn obligations_wrap_report_capture() {
    let (_, demo, clippy, test) = two_obligation_job(CompileDriver::Mbx);
    let steps = names(&demo);
    assert_eq!(&steps[..2], ["Checkout", "Download plan"], "{steps:?}");
    let catalog = ToolCatalog::pinned();
    for task in [&clippy, &test] {
        let name = crate::matrix_step::step_name_for(&task.task_kind, &task.task_id);
        let step = demo
            .steps
            .iter()
            .find(|step| step.name == name)
            .unwrap_or_else(|| panic!("missing task step {name}"));
        assert_task_execution_matches(task, step, &catalog);
    }
}

#[test]
fn obligations_upload_one_artifact_per_job() {
    let (job_id, demo, _, _) = two_obligation_job(CompileDriver::Cargo);
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
