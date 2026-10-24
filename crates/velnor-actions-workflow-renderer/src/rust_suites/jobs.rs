use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, RustSuiteConfig, RustSuiteTrigger, Step,
};

use crate::{RenderError, steps};

use super::RustSuitesRenderSpec;
use super::suite_steps::{
    append_suite_artifact_uploads, compile_driver_steps, install_suite_tools_steps,
    nextest_archive_step, nextest_run_step, producer_mark_steps, restored_rust_step,
};

pub(super) fn ci_trigger_condition(suite: &RustSuiteConfig) -> Option<String> {
    let pr = suite.triggers.contains(&RustSuiteTrigger::PullRequest);
    let push = suite.triggers.contains(&RustSuiteTrigger::Push);
    match (pr, push) {
        (true, false) => Some("github.event_name == 'pull_request'".to_owned()),
        (false, true) => Some("github.event_name == 'push'".to_owned()),
        _ => None,
    }
}

pub(super) fn build_single_suite_job(
    suite: &RustSuiteConfig,
    runs_on: &str,
    timeout: JobTimeout,
    spec: &RustSuitesRenderSpec,
    standalone_workflow: bool,
) -> Result<Job, RenderError> {
    let name = &suite.id;
    let mut job_steps = vec![
        steps::checkout_step(&spec.checkout_uses)?,
        restored_rust_step(spec)?,
    ];
    job_steps.extend(compile_driver_steps(spec)?);
    job_steps.extend(install_suite_tools_steps(spec)?);
    job_steps.push(nextest_run_step(
        suite,
        spec,
        false,
        false,
        standalone_workflow,
    )?);
    job_steps.extend(producer_mark_steps(spec)?);
    append_suite_artifact_uploads(&mut job_steps, suite, false)?;
    Ok(suite_job_record(
        format!("Suite ({name})"),
        runs_on,
        timeout,
        Vec::new(),
        job_steps,
    ))
}

pub(super) fn build_sharded_archive_jobs(
    suite: &RustSuiteConfig,
    runs_on: &str,
    timeout: JobTimeout,
    spec: &RustSuitesRenderSpec,
    standalone_workflow: bool,
) -> Result<Vec<(String, Job)>, RenderError> {
    let name = &suite.id;
    let build_id = format!("suite-build-{name}");
    let run_id = format!("suite-{name}");
    let archive_artifact = format!("velnor-suite-archive-{}-{name}", steps::RUN_KEY_EXPR);
    let archive_dir_expr = format!("${{{{ runner.temp }}}}/velnor/suite-archive-{name}");
    let archive_file_expr = format!("{archive_dir_expr}/archive.tar.zst");

    let mut build_steps = vec![
        steps::checkout_step(&spec.checkout_uses)?,
        restored_rust_step(spec)?,
    ];
    build_steps.extend(compile_driver_steps(spec)?);
    build_steps.extend(install_suite_tools_steps(spec)?);
    build_steps.push(nextest_archive_step(suite, spec)?);
    build_steps.extend(producer_mark_steps(spec)?);
    build_steps.push(steps::upload_suite_archive_step(
        &archive_artifact,
        &archive_file_expr,
    )?);
    let build_job = suite_job_record(
        format!("Build suite ({name})"),
        runs_on,
        timeout,
        Vec::new(),
        build_steps,
    );

    let mut run_steps = vec![
        steps::checkout_step(&spec.checkout_uses)?,
        restored_rust_step(spec)?,
        steps::download_suite_archive_step(&archive_artifact, &archive_dir_expr)?,
    ];
    run_steps.extend(install_suite_tools_steps(spec)?);
    run_steps.push(nextest_run_step(
        suite,
        spec,
        true,
        true,
        standalone_workflow,
    )?);
    append_suite_artifact_uploads(&mut run_steps, suite, true)?;
    let run_job = suite_job_record(
        format!("Suite ({name})"),
        runs_on,
        timeout,
        vec![build_id.clone()],
        run_steps,
    );
    Ok(vec![(build_id, build_job), (run_id, run_job)])
}

pub(super) fn build_sharded_inline_job(
    suite: &RustSuiteConfig,
    runs_on: &str,
    timeout: JobTimeout,
    spec: &RustSuitesRenderSpec,
    standalone_workflow: bool,
) -> Result<Job, RenderError> {
    let name = &suite.id;
    let mut job_steps = vec![
        steps::checkout_step(&spec.checkout_uses)?,
        restored_rust_step(spec)?,
    ];
    job_steps.extend(compile_driver_steps(spec)?);
    job_steps.extend(install_suite_tools_steps(spec)?);
    job_steps.push(nextest_run_step(
        suite,
        spec,
        true,
        false,
        standalone_workflow,
    )?);
    job_steps.extend(producer_mark_steps(spec)?);
    append_suite_artifact_uploads(&mut job_steps, suite, true)?;
    Ok(suite_job_record(
        format!("Suite ({name})"),
        runs_on,
        timeout,
        Vec::new(),
        job_steps,
    ))
}

fn suite_job_record(
    display_name: String,
    runs_on: &str,
    timeout_minutes: JobTimeout,
    needs: Vec<String>,
    steps: Vec<Step>,
) -> Job {
    Job {
        services: Vec::new(),
        display_name,
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes,
        needs,
        condition: None,
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
            actions: PermissionLevel::None,
            packages: PermissionLevel::None,
        }),
        cache_mode: None,
        environment: None,
        steps,
    }
}
