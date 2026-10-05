//! Release workflow rendering and generated-tree assembly.
//!
//! Second render entrypoint: [`render_release_workflow`] turns a validated
//! spec plus fixed argv into `release.yml`, and
//! [`render_release_files`] adds the two effective release-plz configs.

use crate::{
    RenderError, guard, marker,
    release_config::{
        BootstrapReleasePlzConfig, ReleasePlzConfig, render_bootstrap_release_plz_config,
        render_release_plz_config,
    },
    release_gates::{ReleaseConfigBinding, check_release_jobs},
    release_jobs::{ReleaseJobSpec, ReleaseWorkflowSpec},
    release_spec::ReleaseTriggers,
    render::RenderedFile,
    steps,
    yaml::{Yaml, render_yaml},
};

/// Generated release workflow path inside the repository.
pub const RELEASE_WORKFLOW_PATH: &str = ".github/workflows/release.yml";
/// Generated effective normal-policy release-plz config path.
pub const RELEASE_CONFIG_PATH: &str = ".github/release-plz.toml";
/// Generated bootstrap-only release-plz config path.
pub const RELEASE_BOOTSTRAP_CONFIG_PATH: &str = ".github/release-plz-bootstrap.toml";
/// Workspace-relative directory of the exact-source checkout.
///
/// Jobs that run release-plz against the approved source check the
/// policy tree out at the workspace root (event SHA: configs live
/// there) plus the approved source SHA into this directory; release-plz
/// then takes `--config` from the root and `--manifest-path` from here
/// (release contract §11: never dirty the release checkout to insert
/// config). Fixed so gates can bind argv to the source tree.
pub const RELEASE_SOURCE_DIR: &str = "release-source";
/// Every release-owned tree path, sorted.
pub const RELEASE_TREE_PATHS: &[&str] = &[
    RELEASE_BOOTSTRAP_CONFIG_PATH,
    RELEASE_CONFIG_PATH,
    RELEASE_WORKFLOW_PATH,
];

/// Release-owned paths the generator removes when release is disabled.
///
/// The orchestrator's whole-tree swap deletes anything outside the emitted
/// tree; this inventory names the release family for previews and guards.
/// With release disabled the CI tree holds none of these paths.
#[must_use]
pub fn release_stale_paths() -> &'static [&'static str] {
    RELEASE_TREE_PATHS
}

/// Caller-supplied validated scalars the release IR cannot carry.
#[derive(Debug, Clone)]
pub struct ReleaseRenderContext {
    /// Exact generator version for the marker.
    pub generator_version: String,
    /// Single literal versioned Ubuntu label every job must use.
    pub runs_on: String,
}

impl ReleaseRenderContext {
    /// Validate every context scalar before rendering.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] describing the first invalid scalar.
    pub fn validate(&self) -> Result<(), RenderError> {
        marker::validate_version(&self.generator_version)?;
        guard::validate_runs_on(&self.runs_on)
    }
}

/// Render the release workflow document from validated typed IR.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid context, spec, gates, or steps.
pub fn render_release_workflow(
    spec: &ReleaseWorkflowSpec,
    ctx: &ReleaseRenderContext,
) -> Result<String, RenderError> {
    ctx.validate()?;
    spec.validate()?;
    for (id, job) in &spec.jobs {
        if job.runs_on != ctx.runs_on {
            return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
        }
    }
    let binding = ReleaseConfigBinding {
        effective: RELEASE_CONFIG_PATH,
        bootstrap: RELEASE_BOOTSTRAP_CONFIG_PATH,
    };
    check_release_jobs(spec, &binding)?;
    let document = release_document(spec)?;
    let document = crate::yaml::quote_run_values_in_yaml(document);
    let text = marker::with_marker(&ctx.generator_version, &render_yaml(&document))?;
    crate::workflow_size::check_workflow_size(RELEASE_WORKFLOW_PATH, &text)?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

/// Build the release document: name, on, permissions, concurrency, jobs.
fn release_document(spec: &ReleaseWorkflowSpec) -> Result<Yaml, RenderError> {
    let mut jobs = Vec::with_capacity(spec.jobs.len());
    for (id, job) in &spec.jobs {
        jobs.push((id.clone(), release_job_to_yaml(job)?));
    }
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(spec.name.clone())),
        ("on".to_owned(), release_triggers_to_yaml(&spec.triggers)),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str(spec.concurrency.group.clone()),
                ),
                (
                    "cancel-in-progress".to_owned(),
                    Yaml::Bool(spec.concurrency.cancel_in_progress),
                ),
            ]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ]))
}

/// Render release triggers: push, schedule, typed workflow dispatch.
fn release_triggers_to_yaml(triggers: &ReleaseTriggers) -> Yaml {
    let branches: Vec<Yaml> = triggers
        .push_branches
        .iter()
        .map(|branch| Yaml::str(branch.clone()))
        .collect();
    let mut entries = vec![(
        "push".to_owned(),
        Yaml::Map(vec![("branches".to_owned(), Yaml::Seq(branches))]),
    )];
    if let Some(schedule) = &triggers.schedule {
        let crons: Vec<Yaml> = schedule
            .cron
            .iter()
            .map(|cron| Yaml::Map(vec![("cron".to_owned(), Yaml::str(cron.clone()))]))
            .collect();
        entries.push(("schedule".to_owned(), Yaml::Seq(crons)));
    }
    let mut inputs = Vec::with_capacity(triggers.dispatch_inputs.len());
    for input in &triggers.dispatch_inputs {
        let mut fields = vec![
            (
                "description".to_owned(),
                Yaml::str(input.description.clone()),
            ),
            ("required".to_owned(), Yaml::Bool(input.required)),
        ];
        if let Some(default) = &input.default {
            fields.push(("default".to_owned(), Yaml::str(default.clone())));
        }
        inputs.push((input.name.clone(), Yaml::Map(fields)));
    }
    entries.push((
        "workflow_dispatch".to_owned(),
        Yaml::Map(vec![("inputs".to_owned(), Yaml::Map(inputs))]),
    ));
    Yaml::Map(entries)
}

/// Render one release job with environment, permissions, needs, steps.
fn release_job_to_yaml(job: &ReleaseJobSpec) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(&job.display_name)?;
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(job.display_name.clone())),
        ("runs-on".to_owned(), Yaml::str(job.runs_on.clone())),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(job.timeout_minutes.minutes())),
        ),
    ];
    if let Some(environment) = &job.environment {
        entries.push(("environment".to_owned(), Yaml::str(environment.clone())));
    }
    entries.push((
        "permissions".to_owned(),
        Yaml::Map(vec![
            (
                "contents".to_owned(),
                Yaml::str(job.permissions.contents.as_str()),
            ),
            (
                "pull-requests".to_owned(),
                Yaml::str(job.permissions.pull_requests.as_str()),
            ),
            (
                "id-token".to_owned(),
                Yaml::str(job.permissions.id_token.as_str()),
            ),
        ]),
    ));
    if !job.needs.is_empty() {
        let needs: Vec<Yaml> = job
            .needs
            .iter()
            .map(|need| Yaml::str(need.clone()))
            .collect();
        entries.push(("needs".to_owned(), Yaml::Seq(needs)));
    }
    if let Some(condition) = &job.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    let mut rendered = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        rendered.push(crate::steps_plain::plain_step_to_yaml(step)?);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered)));
    Ok(Yaml::Map(entries))
}

/// The three rendered release files with their fixed tree paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFiles {
    /// `release.yml` workflow bytes.
    pub workflow: RenderedFile,
    /// Effective normal-policy config bytes.
    pub config: RenderedFile,
    /// Bootstrap-only config bytes.
    pub bootstrap_config: RenderedFile,
}

impl ReleaseFiles {
    /// Borrow the three files as a sorted vector for tree assembly.
    #[must_use]
    pub fn as_sorted_vec(&self) -> Vec<RenderedFile> {
        let mut files = vec![
            self.workflow.clone(),
            self.config.clone(),
            self.bootstrap_config.clone(),
        ];
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files
    }
}

/// Render the workflow plus both effective configs with fixed paths.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid specs, configs, or paths.
pub fn render_release_files(
    spec: &ReleaseWorkflowSpec,
    ctx: &ReleaseRenderContext,
    config: &ReleasePlzConfig,
    bootstrap: &BootstrapReleasePlzConfig,
) -> Result<ReleaseFiles, RenderError> {
    let workflow = render_release_workflow(spec, ctx)?;
    let normal = render_release_plz_config(config, &ctx.generator_version)?;
    let bootstrap_text = render_bootstrap_release_plz_config(bootstrap, &ctx.generator_version)?;
    let files = ReleaseFiles {
        workflow: RenderedFile {
            path: RELEASE_WORKFLOW_PATH.to_owned(),
            bytes: workflow,
        },
        config: RenderedFile {
            path: RELEASE_CONFIG_PATH.to_owned(),
            bytes: normal,
        },
        bootstrap_config: RenderedFile {
            path: RELEASE_BOOTSTRAP_CONFIG_PATH.to_owned(),
            bytes: bootstrap_text,
        },
    };
    check_release_paths(&files)?;
    Ok(files)
}

/// Validate the release paths against the fixed release allowlist.
fn check_release_paths(files: &ReleaseFiles) -> Result<(), RenderError> {
    guard::validate_allowlisted_path(&files.workflow.path, &[RELEASE_WORKFLOW_PATH])?;
    guard::validate_allowlisted_path(&files.config.path, &[RELEASE_CONFIG_PATH])?;
    guard::validate_allowlisted_path(
        &files.bootstrap_config.path,
        &[RELEASE_BOOTSTRAP_CONFIG_PATH],
    )?;
    Ok(())
}
