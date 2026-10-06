//! Release workflow rendering and generated-tree assembly.
//!
//! Second render entrypoint: [`render_release_workflow`] turns a validated
//! spec plus fixed argv into `release.yml`, and
//! [`render_release_files`] adds the complete immutable helper source family.

use crate::{
    RenderError, guard, marker,
    release_gates::check_release_jobs,
    release_jobs::{ReleaseJobSpec, ReleaseWorkflowSpec},
    release_spec::ReleaseTriggers,
    render::RenderedFile,
    steps,
    yaml::{Yaml, render_yaml},
};

/// Generated release workflow path inside the repository.
pub const RELEASE_WORKFLOW_PATH: &str = ".github/workflows/release.yml";
/// Workspace-relative directory of the exact-source checkout.
///
/// Anonymous package and preparation helpers use the approved source SHA in
/// this fixed directory. Credentialed publisher jobs consume verified artifacts.
pub const RELEASE_SOURCE_DIR: &str = "release-source";
/// Fixed credential-free package helper path.
pub const PACKAGE_SCRIPT_PATH: &str = ".github/velnor/release_package.sh";
/// Fixed credentialed forge preflight helper path.
pub const FORGE_PREFLIGHT_SCRIPT_PATH: &str = ".github/velnor/release_forge_preflight.sh";
/// Fixed reconciliation launcher path.
pub const RECONCILE_SCRIPT_PATH: &str = ".github/velnor/release_reconcile.sh";
/// Every release-owned tree path, sorted.
pub const RELEASE_TREE_PATHS: &[&str] = &[
    ".github/velnor/release_admission.py",
    ".github/velnor/release_forge_preflight.py",
    ".github/velnor/release_forge_preflight.sh",
    ".github/velnor/release_forge_publish.py",
    ".github/velnor/release_forge_publish.sh",
    ".github/velnor/release_forge_publish_api.py",
    ".github/velnor/release_forge_publish_read.py",
    ".github/velnor/release_forge_publish_verify.py",
    ".github/velnor/release_original_source_origin.py",
    ".github/velnor/release_original_source_origin_context.py",
    ".github/velnor/release_package.py",
    ".github/velnor/release_package.sh",
    ".github/velnor/release_package_contract.py",
    ".github/velnor/release_preflight_cargo.py",
    ".github/velnor/release_prepare_anonymous.py",
    ".github/velnor/release_prepare_anonymous.sh",
    ".github/velnor/release_prepare_bytes.py",
    ".github/velnor/release_prepare_forge.py",
    ".github/velnor/release_prepare_forge.sh",
    ".github/velnor/release_prepare_forge_metadata.py",
    ".github/velnor/release_prepare_forge_workspace.py",
    ".github/velnor/release_prepare_notes.py",
    ".github/velnor/release_prepare_summary.py",
    ".github/velnor/release_publish_artifact.py",
    ".github/velnor/release_publish_auth.py",
    ".github/velnor/release_publish_entry.py",
    ".github/velnor/release_publish_manifest.py",
    ".github/velnor/release_publish_metadata.py",
    ".github/velnor/release_publish_proof.py",
    ".github/velnor/release_publish_registry.py",
    ".github/velnor/release_publish_transport.py",
    ".github/velnor/release_publish_verify.py",
    ".github/velnor/release_reconcile.sh",
    ".github/velnor/release_reconcile_cargo.py",
    ".github/velnor/release_reconcile_common.py",
    ".github/velnor/release_reconcile_entry.py",
    ".github/velnor/release_reconcile_forge.py",
    ".github/velnor/release_reconcile_registry.py",
    ".github/velnor/release_registry_artifact_proof.sh",
    ".github/velnor/release_registry_publish.sh",
    ".github/velnor/release_source_snapshot.py",
    ".github/velnor/release_source_snapshot.sh",
    ".github/velnor/release_source_snapshot_entry.py",
    ".github/velnor/release_source_snapshot_output.py",
    ".github/velnor/release_source_tree.py",
    ".github/velnor/release_source_validation.py",
    ".github/workflows/release.yml",
];

/// Retired launcher paths accepted only for marker-proven stale removal.
pub const RELEASE_RETIRED_TREE_PATHS: &[&str] = &[
    ".github/release-plz-bootstrap.toml",
    ".github/release-plz.toml",
    ".github/velnor/release_preflight.py",
    ".github/velnor/release_preflight.sh",
    ".github/velnor/release_reconcile.py",
    ".github/velnor/release_reconcile_preflight.py",
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
    crate::source_helper::validate_registry(&spec.helper_registry, &ctx.generator_version)?;
    for (id, job) in &spec.jobs {
        if job.runs_on != ctx.runs_on {
            return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
        }
    }
    check_release_jobs(spec)?;
    let document = release_document(spec, &ctx.generator_version)?;
    let document = crate::yaml::quote_run_values_in_yaml(document);
    let text = marker::with_marker(&ctx.generator_version, &render_yaml(&document))?;
    steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

/// Build the release document: name, on, permissions, concurrency, jobs.
fn release_document(spec: &ReleaseWorkflowSpec, version: &str) -> Result<Yaml, RenderError> {
    let mut jobs = Vec::with_capacity(spec.jobs.len());
    for (id, job) in &spec.jobs {
        jobs.push((
            id.clone(),
            release_job_to_yaml(job, &spec.helper_registry, version)?,
        ));
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
fn release_job_to_yaml(
    job: &ReleaseJobSpec,
    records: &[velnor_actions_contract::CompiledSourceHelper],
    version: &str,
) -> Result<Yaml, RenderError> {
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
                "actions".to_owned(),
                Yaml::str(job.permissions.actions.as_str()),
            ),
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
    entries.push((
        "outputs".to_owned(),
        Yaml::Map(
            job.outputs
                .iter()
                .map(|output| (output.name.clone(), Yaml::str(output.value.expression())))
                .collect(),
        ),
    ));
    if let Some(condition) = &job.condition {
        steps::scan_for_private_subcommands(condition)?;
        entries.push(("if".to_owned(), Yaml::str(condition.clone())));
    }
    let mut rendered = Vec::with_capacity(job.steps.len());
    for step in &job.steps {
        let yaml = if matches!(
            step.kind,
            velnor_actions_contract::StepKind::SourceBoundHelper { .. }
        ) {
            crate::source_helper::source_helper_step_to_yaml(step, records, version, &job.runs_on)?
        } else {
            crate::steps_plain::plain_step_to_yaml(step)?
        };
        rendered.push(yaml);
    }
    entries.push(("steps".to_owned(), Yaml::Seq(rendered)));
    Ok(Yaml::Map(entries))
}

/// Complete release family, including fixed protected interpreter support files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFiles {
    /// `release.yml` workflow bytes.
    pub workflow: RenderedFile,
    /// Generator-owned fixed Python helpers and receipt launchers.
    pub helpers: Vec<RenderedFile>,
}

impl ReleaseFiles {
    /// Borrow the complete family as a sorted vector for tree assembly.
    #[must_use]
    pub fn as_sorted_vec(&self) -> Vec<RenderedFile> {
        let mut files = vec![self.workflow.clone()];
        files.extend(self.helpers.iter().cloned());
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files
    }
}

/// Render the workflow and its complete immutable source-owned helper family.
///
/// # Errors
///
/// Returns [`RenderError`] for invalid specs, configs, or paths.
pub fn render_release_files(
    spec: &ReleaseWorkflowSpec,
    ctx: &ReleaseRenderContext,
) -> Result<ReleaseFiles, RenderError> {
    let workflow = render_release_workflow(spec, ctx)?;
    let files = ReleaseFiles {
        workflow: RenderedFile {
            path: RELEASE_WORKFLOW_PATH.to_owned(),
            bytes: workflow,
        },
        helpers: support_files(&ctx.generator_version, &spec.support_sources)?,
    };
    check_release_paths(&files)?;
    Ok(files)
}

/// Emit fixed sources with exactly the same marker bytes as shared native helpers.
fn support_files(
    version: &str,
    owned: &[velnor_actions_contract::CompiledSupportSource],
) -> Result<Vec<RenderedFile>, RenderError> {
    let mut files = Vec::with_capacity(owned.len());
    for source in owned {
        marker::check_first_line(source.source(), version)?;
        files.push(RenderedFile {
            path: source.path().to_owned(),
            bytes: source.source().to_owned(),
        });
    }
    Ok(files)
}

/// Validate the release paths against the fixed release allowlist.
fn check_release_paths(files: &ReleaseFiles) -> Result<(), RenderError> {
    guard::validate_allowlisted_path(&files.workflow.path, &[RELEASE_WORKFLOW_PATH])?;
    for file in &files.helpers {
        guard::validate_allowlisted_path(&file.path, RELEASE_TREE_PATHS)?;
    }
    let paths = files
        .as_sorted_vec()
        .into_iter()
        .map(|file| file.path)
        .collect::<Vec<_>>();
    if paths
        .iter()
        .map(String::as_str)
        .ne(RELEASE_TREE_PATHS.iter().copied())
    {
        return Err(RenderError::InvalidWorkflow(
            "release_helper_inventory".to_owned(),
        ));
    }
    Ok(())
}
