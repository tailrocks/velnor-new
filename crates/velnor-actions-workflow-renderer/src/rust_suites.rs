//! Generic Rust test suite job and workflow builders.
//!
//! Renders `[[stacks.rust.suites]]` into `ci.yml` jobs or standalone
//! `.github/workflows/<slug>.yml` documents (`suites.yml` by default).

use std::collections::BTreeMap;

mod jobs;

use velnor_actions_contract::{
    DEFAULT_SUITE_WORKFLOW_SLUG, Job, JobTimeout, PullRequestCachePolicy, RunsOn,
    RustSuiteBuildMode, RustSuiteConfig, Step, WorkflowDispatchConfig, validate_rust_suites,
    validate_suites_with_dispatch,
};

use crate::workflow_routing::WorkflowRoutingPolicy;
use crate::{CompileDriver, MiseSetup, RenderError, RenderedFile, steps};

#[path = "rust_suites_document.rs"]
mod document;
#[path = "rust_suites_steps.rs"]
mod suite_steps;
#[cfg(test)]
#[path = "rust_suites_tests.rs"]
mod tests;

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
    render_suites_workflow_with_helpers(spec, &BTreeMap::new())
}

/// Render the default suite document with verified helper acquisition steps.
///
/// Map keys are exact runner labels. A job without a matching helper keeps
/// its pinned Mise setup and omits cache actions that require the helper.
/// # Errors
pub fn render_suites_workflow_with_helpers(
    spec: &RustSuitesRenderSpec,
    helper_steps: &BTreeMap<String, Step>,
) -> Result<Option<String>, RenderError> {
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
    document::render_workflow_for_slug(
        DEFAULT_SUITE_WORKFLOW_SLUG,
        &entries,
        spec,
        &WorkflowRoutingPolicy::Preserve,
        helper_steps,
    )
    .map(Some)
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
    render_suites_workflows_with_routing(spec, &WorkflowRoutingPolicy::Preserve)
}

/// Render standalone suite workflows under the generator's effective placement policy.
///
/// # Errors
///
/// Invalid suite configuration, unsupported runners, and workflow rendering errors fail.
pub fn render_suites_workflows_with_routing(
    spec: &RustSuitesRenderSpec,
    routing: &WorkflowRoutingPolicy,
) -> Result<Vec<RenderedFile>, RenderError> {
    render_suites_workflows_with_routing_and_helpers(spec, routing, &BTreeMap::new())
}

/// Render standalone suite workflows with verified helper steps by runner label.
///
/// Jobs without a helper retain their pinned Mise setup but omit cache actions
/// that require the staged helper.
/// # Errors
pub fn render_suites_workflows_with_routing_and_helpers(
    spec: &RustSuitesRenderSpec,
    routing: &WorkflowRoutingPolicy,
    helper_steps: &BTreeMap<String, Step>,
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
        let yaml = document::render_workflow_for_slug(slug, &entries, spec, routing, helper_steps)?;
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
        .then(|| jobs::ci_trigger_condition(suite))
        .flatten();
    let mut jobs = if suite.partitions == 1 {
        let job = jobs::build_single_suite_job(suite, runs_on, timeout, spec, standalone_workflow)?;
        vec![(format!("suite-{name}"), job)]
    } else {
        match suite.build {
            RustSuiteBuildMode::Archive => jobs::build_sharded_archive_jobs(
                suite,
                runs_on,
                timeout,
                spec,
                standalone_workflow,
            )?,
            RustSuiteBuildMode::Inline => {
                let job = jobs::build_sharded_inline_job(
                    suite,
                    runs_on,
                    timeout,
                    spec,
                    standalone_workflow,
                )?;
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
