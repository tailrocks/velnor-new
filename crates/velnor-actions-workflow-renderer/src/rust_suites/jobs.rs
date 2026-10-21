use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, RustSuiteConfig, RustSuiteTrigger, Step,
};

use crate::{RenderError, steps};

use super::RustSuitesRenderSpec;
use super::suite_steps::{
    compile_driver_steps, install_suite_tools_step, nextest_archive_step, nextest_run_step,
    producer_mark_steps,
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
    let mut job_steps = vec![steps::checkout_step(&spec.checkout_uses)?];
    job_steps.extend(compile_driver_steps(spec)?);
    job_steps.push(install_suite_tools_step(spec)?);
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

    let mut build_steps = vec![steps::checkout_step(&spec.checkout_uses)?];
    build_steps.extend(compile_driver_steps(spec)?);
    build_steps.push(install_suite_tools_step(spec)?);
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
        steps::download_suite_archive_step(&archive_artifact, &archive_dir_expr)?,
        install_suite_tools_step(spec)?,
        nextest_run_step(suite, spec, true, true, standalone_workflow)?,
    ];
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
    let mut job_steps = vec![steps::checkout_step(&spec.checkout_uses)?];
    job_steps.extend(compile_driver_steps(spec)?);
    job_steps.push(install_suite_tools_step(spec)?);
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

fn append_suite_artifact_uploads(
    job_steps: &mut Vec<Step>,
    suite: &RustSuiteConfig,
    sharded: bool,
) -> Result<(), RenderError> {
    let name = &suite.id;
    let id_prefix = format!("{name}-");
    for artifact in &suite.artifacts {
        let resolved_label = artifact
            .name
            .replace("{shard}", if sharded { "${{ matrix.shard }}" } else { "1" })
            .replace(
                "{partition}",
                if sharded { "${{ matrix.shard }}" } else { "1" },
            );
        let has_id_prefix = resolved_label == *name || resolved_label.starts_with(&id_prefix);
        let needs_shard_suffix = sharded && !resolved_label.contains("${{ matrix.shard }}");
        let artifact_name = match (has_id_prefix, needs_shard_suffix) {
            (true, true) => format!(
                "velnor-suite-{}-{resolved_label}-s${{{{ matrix.shard }}}}",
                steps::RUN_KEY_EXPR
            ),
            (true, false) => format!("velnor-suite-{}-{resolved_label}", steps::RUN_KEY_EXPR),
            (false, true) => format!(
                "velnor-suite-{}-{name}-s${{{{ matrix.shard }}}}-{resolved_label}",
                steps::RUN_KEY_EXPR
            ),
            (false, false) => format!(
                "velnor-suite-{}-{name}-{resolved_label}",
                steps::RUN_KEY_EXPR
            ),
        };
        let path = artifact.paths.join("\n");
        job_steps.push(steps::upload_suite_artifact_step(
            &format!("Upload suite artifact ({})", artifact.name),
            &artifact_name,
            &path,
            u32::from(artifact.retention_days),
            artifact.when,
        )?);
    }
    Ok(())
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
