//! Generic Rust test suite job and workflow builders.
//!
//! Renders `[[stacks.rust.suites]]` into `ci.yml` jobs or standalone
//! `.github/workflows/<slug>.yml` documents (`suites.yml` by default).

use std::collections::BTreeMap;

use velnor_actions_contract::{
    DEFAULT_SUITE_WORKFLOW_SLUG, Job, JobTimeout, PermissionLevel, Permissions,
    PullRequestCachePolicy, RunsOn, RustSuiteBuildMode, RustSuiteConfig, RustSuiteTrigger, Step,
    WorkflowDispatchConfig, validate_rust_suites, validate_suites_with_dispatch,
};

use crate::{CompileDriver, MiseSetup, RenderError, RenderedFile, steps};

#[path = "rust_suites_document.rs"]
mod document;
#[path = "rust_suites_steps.rs"]
mod suite_steps;
#[cfg(test)]
#[path = "rust_suites_tests.rs"]
mod tests;
use suite_steps::{
    compile_driver_steps, install_suite_tools_steps, nextest_archive_step, nextest_run_step,
    restored_rust_step,
};

/// Specification for rendering declared Rust test suite jobs and workflows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustSuitesRenderSpec {
    /// Declared Rust test suites in declaration order.
    pub suites: Vec<RustSuiteConfig>,
    /// Optional custom `workflow_dispatch` inputs for suite workflows.
    pub workflow_dispatch: Option<WorkflowDispatchConfig>,
    /// Default runner label when a suite omits `runner_label`.
    pub default_runs_on: String,
    /// Full-SHA `actions/checkout` reference.
    pub checkout_uses: String,
    /// Renderer generator version for the header marker.
    pub generator_version: String,
    /// Pinned Mise setup for the default runner target.
    pub mise: MiseSetup,
    /// Pinned Rust toolchain version (e.g., `"1.95.0"`).
    pub rust_version: String,
    /// Pinned `cargo-nextest` tool version (e.g., `"0.9.133"`).
    pub cargo_nextest_version: String,
    /// Compile driver (`Cargo` or `Mbx`) for suite compilation steps.
    pub compile_driver: CompileDriver,
    /// Optional MBX action `(uses, version)` when `compile_driver` is `CompileDriver::Mbx`.
    pub mbx_action: Option<(String, String)>,
    /// Pull-request cache save policy.
    pub pull_request_cache_policy: PullRequestCachePolicy,
    /// `sh -c` argv run right after the tools cache restore: discards the
    /// cached rustup and verifies a restored toolchain file by file.
    pub verify_restored_rust_argv: Vec<String>,
    /// `sh -c` argv run right after the suite install: verifies the
    /// installed toolchain file by file.
    pub verify_rust_argv: Vec<String>,
    /// Whether step environments export `RUSTUP_TOOLCHAIN`.
    pub export_rustup_toolchain: bool,
}

/// Build suite jobs triggered on `pull_request` or `push` for merging into `ci.yml`.
///
/// `finalize_jobs` attaches V2 tools-cache setup, writer election, and cleanup.
///
/// # Errors
///
/// Returns [`RenderError`] when `spec` or any suite configuration is invalid.
pub fn build_ci_suite_jobs(
    spec: &RustSuitesRenderSpec,
) -> Result<BTreeMap<String, Job>, RenderError> {
    validate_spec(spec)?;
    let mut jobs = BTreeMap::new();
    for suite in &spec.suites {
        if !suite.has_pr_or_push_trigger() {
            continue;
        }
        for (job_id, job) in build_suite_jobs_for_entry(suite, spec, false)? {
            jobs.insert(job_id, job);
        }
    }
    Ok(jobs)
}

/// Render the default `.github/workflows/suites.yml` document when any suite
/// targets the default `"suites"` workflow slug with `workflow_dispatch` or `schedule`.
///
/// # Errors
///
/// Returns [`RenderError`] when `spec` or any suite configuration is invalid.
pub fn render_suites_workflow(spec: &RustSuitesRenderSpec) -> Result<Option<String>, RenderError> {
    validate_spec(spec)?;
    let entries: Vec<&RustSuiteConfig> = spec
        .suites
        .iter()
        .filter(|suite| {
            suite_workflow_slug(suite) == DEFAULT_SUITE_WORKFLOW_SLUG
                && has_standalone_trigger(suite)
        })
        .collect();
    if entries.is_empty() {
        return Ok(None);
    }
    document::render_workflow_for_slug(DEFAULT_SUITE_WORKFLOW_SLUG, &entries, spec).map(Some)
}

/// Render all standalone `.github/workflows/<slug>.yml` suite workflow files,
/// grouped by workflow slug.
///
/// # Errors
///
/// Returns [`RenderError`] when `spec` or any suite configuration is invalid.
pub fn render_suites_workflows(
    spec: &RustSuitesRenderSpec,
) -> Result<Vec<RenderedFile>, RenderError> {
    validate_spec(spec)?;
    let mut grouped: BTreeMap<&str, Vec<&RustSuiteConfig>> = BTreeMap::new();
    for suite in &spec.suites {
        if has_standalone_trigger(suite) {
            grouped
                .entry(suite_workflow_slug(suite))
                .or_default()
                .push(suite);
        }
    }
    let mut files = Vec::with_capacity(grouped.len());
    for (slug, entries) in grouped {
        let yaml = document::render_workflow_for_slug(slug, &entries, spec)?;
        files.push(RenderedFile {
            path: format!(".github/workflows/{slug}.yml"),
            bytes: yaml,
        });
    }
    Ok(files)
}

pub(super) fn suite_workflow_slug(suite: &RustSuiteConfig) -> &str {
    suite
        .workflow
        .as_deref()
        .unwrap_or(DEFAULT_SUITE_WORKFLOW_SLUG)
}

pub(super) fn has_standalone_trigger(suite: &RustSuiteConfig) -> bool {
    suite.has_workflow_dispatch_trigger() || suite.has_schedule_trigger()
}

pub(super) fn validate_spec(spec: &RustSuitesRenderSpec) -> Result<(), RenderError> {
    const CONFIG_FILE: &str = ".velnor/config.toml";
    spec.mise.validate()?;
    crate::marker::validate_version(&spec.generator_version)?;
    RunsOn::parse(&spec.default_runs_on).map_err(RenderError::Contract)?;
    steps::validate_uses(&spec.checkout_uses)?;
    if !spec.checkout_uses.starts_with("actions/checkout@") {
        return Err(RenderError::BadActionRef(format!(
            "not_checkout:{}",
            spec.checkout_uses
        )));
    }
    if !crate::cache_p08::is_catalog_version(&spec.rust_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_rust_version:{}",
            spec.rust_version
        )));
    }
    if !crate::cache_p08::is_catalog_version(&spec.cargo_nextest_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_nextest_version:{}",
            spec.cargo_nextest_version
        )));
    }
    validate_rust_suites(&spec.suites, CONFIG_FILE).map_err(RenderError::Contract)?;
    let empty_dispatch = WorkflowDispatchConfig::default();
    let dispatch = spec.workflow_dispatch.as_ref().unwrap_or(&empty_dispatch);
    dispatch
        .validate(CONFIG_FILE)
        .map_err(RenderError::Contract)?;
    validate_suites_with_dispatch(&spec.suites, dispatch, CONFIG_FILE)
        .map_err(RenderError::Contract)
}

pub(super) fn build_suite_jobs_for_entry(
    suite: &RustSuiteConfig,
    spec: &RustSuitesRenderSpec,
    standalone_workflow: bool,
) -> Result<Vec<(String, Job)>, RenderError> {
    let name = &suite.id;
    let runs_on = suite
        .runner_label
        .as_deref()
        .unwrap_or(&spec.default_runs_on);
    RunsOn::parse(runs_on).map_err(RenderError::Contract)?;
    let timeout = JobTimeout::new(suite.timeout_minutes).map_err(RenderError::Contract)?;
    let ci_condition = (!standalone_workflow)
        .then(|| ci_trigger_condition(suite))
        .flatten();
    let mut jobs = if suite.partitions == 1 {
        let job = build_single_suite_job(suite, runs_on, timeout, spec, standalone_workflow)?;
        vec![(format!("suite-{name}"), job)]
    } else {
        match suite.build {
            RustSuiteBuildMode::Archive => {
                build_sharded_archive_jobs(suite, runs_on, timeout, spec, standalone_workflow)?
            }
            RustSuiteBuildMode::Inline => {
                let job =
                    build_sharded_inline_job(suite, runs_on, timeout, spec, standalone_workflow)?;
                vec![(format!("suite-{name}"), job)]
            }
        }
    };
    if ci_condition.is_some() {
        for (_, job) in &mut jobs {
            job.condition.clone_from(&ci_condition);
        }
    }
    Ok(jobs)
}

fn ci_trigger_condition(suite: &RustSuiteConfig) -> Option<String> {
    let pr = suite.triggers.contains(&RustSuiteTrigger::PullRequest);
    let push = suite.triggers.contains(&RustSuiteTrigger::Push);
    match (pr, push) {
        (true, false) => Some("github.event_name == 'pull_request'".to_owned()),
        (false, true) => Some("github.event_name == 'push'".to_owned()),
        _ => None,
    }
}

fn build_single_suite_job(
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
    job_steps.extend(suite_steps::producer_mark_steps(spec)?);
    append_suite_artifact_uploads(&mut job_steps, suite, false)?;
    Ok(suite_job_record(
        format!("Suite ({name})"),
        runs_on,
        timeout,
        Vec::new(),
        job_steps,
    ))
}

fn build_sharded_archive_jobs(
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
    build_steps.extend(suite_steps::producer_mark_steps(spec)?);
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

fn build_sharded_inline_job(
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
    job_steps.extend(suite_steps::producer_mark_steps(spec)?);
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
