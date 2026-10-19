//! Orchestrator validation and emission for declared `[[stacks.rust.suites]]`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_actionlint::PinnedActionRef;
use velnor_actions_actionlint::actions::{MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION};
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::{Job, RunsOn, RustSuiteConfig, VelnorConfig};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_rust::CompileDriver as RustCompileDriver;
use velnor_actions_workflow_renderer::rust_suites::{
    RustSuitesRenderSpec, build_ci_suite_jobs, render_suites_workflows_with_routing_and_helpers,
};
use velnor_actions_workflow_renderer::{CompileDriver, RenderedFile, WorkflowRoutingPolicy};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::{Discovery, PlannedWorkspace, workspace_manifest};
use crate::pins::resolve_mise_setup;
use crate::prepare::GenerationPreparation;
use crate::workflow::CHECKOUT_USES;

/// Validate declared Rust suites against discovered workspaces, packages, and Nextest profiles.
pub(crate) fn validate_suites_against_repo(
    root: &Path,
    config: &VelnorConfig,
    discovery: &Discovery,
) -> Result<(), OrchestratorError> {
    let Some(rust) = &config.stacks.rust else {
        return Ok(());
    };
    if rust.suites.is_empty() {
        return Ok(());
    }
    if discovery.workspaces.is_empty() {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.suites",
            "rust_suites_without_workspace",
        ));
    }
    for (idx, suite) in rust.suites.iter().enumerate() {
        validate_single_suite(root, idx, suite, &discovery.workspaces)?;
    }
    Ok(())
}

fn find_suite_workspace<'a>(
    manifest_path: Option<&str>,
    workspaces: &'a [PlannedWorkspace],
) -> Option<&'a PlannedWorkspace> {
    match manifest_path {
        Some(manifest) => workspaces.iter().find(|ws| {
            workspace_manifest(&ws.record.workspace_root) == manifest
                || ws
                    .record
                    .packages
                    .iter()
                    .any(|pkg| pkg.in_workspace && !pkg.external && pkg.manifest == manifest)
        }),
        None => workspaces.first(),
    }
}

fn validate_single_suite(
    root: &Path,
    idx: usize,
    suite: &RustSuiteConfig,
    workspaces: &[PlannedWorkspace],
) -> Result<(), OrchestratorError> {
    let workspace =
        find_suite_workspace(suite.manifest_path.as_deref(), workspaces).ok_or_else(|| {
            OrchestratorError::config(
                CONFIG_REL,
                format!("stacks.rust.suites[{idx}].manifest_path"),
                format!(
                    "unknown_manifest_path:{}",
                    suite.manifest_path.as_deref().unwrap_or_default()
                ),
            )
        })?;
    if let Some(pkg) = &suite.package
        && !workspace
            .record
            .packages
            .iter()
            .any(|record| record.in_workspace && !record.external && record.name == *pkg)
    {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            format!("stacks.rust.suites[{idx}].package"),
            format!("unknown_package:{pkg}"),
        ));
    }
    if suite.profile != "default" {
        let inputs = crate::evidence::nextest_inputs(root, &workspace.record.workspace_root)?;
        let declared = inputs
            .iter()
            .any(|cfg| cfg.profiles.iter().any(|profile| profile == &suite.profile));
        if !declared {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                format!("stacks.rust.suites[{idx}].profile"),
                format!("unknown_nextest_profile:{}", suite.profile),
            ));
        }
    }
    Ok(())
}

/// Build `ci.yml` suite jobs and the subset of job IDs that gate `Required`.
pub(crate) fn build_ci_jobs(
    root: &Path,
    config: &VelnorConfig,
    label: &str,
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<(BTreeMap<String, Job>, Vec<String>), OrchestratorError> {
    validate_suites_against_repo(root, config, discovery)?;
    let Some(spec) = build_render_spec(config, label, discovery, catalog)? else {
        return Ok((BTreeMap::new(), Vec::new()));
    };
    let mut ci_jobs = build_ci_suite_jobs(&spec)?;
    if config.workflow.policy == WorkflowPolicy::ConsumerV1 {
        for job in ci_jobs.values_mut() {
            let release_label = helper_release_label(&job.runs_on);
            let acquire = crate::pins::consumer_acquire_step(
                &release_label,
                env!("CARGO_PKG_VERSION"),
                discovery,
            )?;
            job.steps.insert(1.min(job.steps.len()), acquire);
        }
    }
    let mut required_ids = Vec::new();
    for suite in &spec.suites {
        if suite.has_pr_or_push_trigger() && suite.required {
            let build_id = format!("suite-build-{}", suite.id);
            if ci_jobs.contains_key(&build_id) {
                required_ids.push(build_id);
            }
            let run_id = format!("suite-{}", suite.id);
            if ci_jobs.contains_key(&run_id) {
                required_ids.push(run_id);
            }
        }
    }
    Ok((ci_jobs, required_ids))
}

/// Render standalone `.github/workflows/<slug>.yml` suite workflow files.
pub(crate) fn suite_workflow_files(
    prep: &GenerationPreparation,
    routing: &WorkflowRoutingPolicy,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    validate_suites_against_repo(&prep.root, &prep.config, &prep.discovery)?;
    let catalog = &prep.emission_catalog();
    let Some(spec) = build_render_spec(&prep.config, &prep.runner_label, &prep.discovery, catalog)?
    else {
        return Ok(Vec::new());
    };
    let helper_steps = standalone_helper_steps(prep, &spec)?;
    Ok(render_suites_workflows_with_routing_and_helpers(
        &spec,
        routing,
        &helper_steps,
    )?)
}

fn standalone_helper_steps(
    prep: &GenerationPreparation,
    spec: &RustSuitesRenderSpec,
) -> Result<BTreeMap<String, velnor_actions_contract::Step>, OrchestratorError> {
    let mut labels = BTreeSet::new();
    for suite in &spec.suites {
        if suite.has_workflow_dispatch_trigger() || suite.has_schedule_trigger() {
            labels.insert(
                suite
                    .runner_label
                    .as_deref()
                    .unwrap_or(&spec.default_runs_on)
                    .to_owned(),
            );
        }
    }
    let mut steps = BTreeMap::new();
    match prep.config.workflow.policy {
        WorkflowPolicy::ConsumerV1 => {
            for label in labels {
                let step = crate::pins::consumer_acquire_step(
                    &helper_release_label(&label),
                    env!("CARGO_PKG_VERSION"),
                    &prep.discovery,
                )?;
                steps.insert(label, step);
            }
        }
        WorkflowPolicy::VelnorRepositoryV1 => {
            if let Some(lock) = crate::validate::verify_velnor_repository_files(&prep.root)? {
                let staged = format!(
                    "{}{}",
                    velnor_actions_workflow_renderer::STAGED_BINARY_PREFIX,
                    env!("CARGO_PKG_VERSION")
                );
                for label in labels {
                    let step = crate::pins::lock_acquire_step(
                        &lock,
                        &helper_release_label(&label),
                        &staged,
                    )?;
                    steps.insert(label, step);
                }
            }
        }
    }
    Ok(steps)
}

pub(crate) fn helper_release_label(runner_label: &str) -> String {
    if RunsOn::parse(runner_label).is_ok_and(|runs_on| runs_on.is_scale_set()) {
        "ubuntu-26.04".to_owned()
    } else {
        runner_label.to_owned()
    }
}

fn build_render_spec(
    config: &VelnorConfig,
    label: &str,
    discovery: &Discovery,
    catalog: &ToolCatalog,
) -> Result<Option<RustSuitesRenderSpec>, OrchestratorError> {
    let Some(rust) = &config.stacks.rust else {
        return Ok(None);
    };
    if rust.suites.is_empty() {
        return Ok(None);
    }
    let first_manifest = rust.suites.first().and_then(|s| s.manifest_path.as_deref());
    let primary = find_suite_workspace(first_manifest, &discovery.workspaces)
        .or_else(|| discovery.workspaces.first())
        .ok_or_else(|| {
            OrchestratorError::config(
                CONFIG_REL,
                "stacks.rust.suites",
                "rust_suites_without_workspace",
            )
        })?;
    let (compile_driver, mbx_action) = match primary.profile.compile_driver {
        RustCompileDriver::Cargo => (CompileDriver::Cargo, None),
        RustCompileDriver::Mbx => {
            let uses = PinnedActionRef::new(
                "jdx/mr-boxington-action",
                None,
                MR_BOXINGTON_ACTION_SHA,
                MR_BOXINGTON_ACTION_VERSION,
            )?
            .uses_value();
            (
                CompileDriver::Mbx,
                Some((uses, catalog.version(PinnedTool::MrBoxington).to_owned())),
            )
        }
    };
    let workflow_dispatch =
        (!config.workflow.dispatch.is_empty()).then(|| config.workflow.dispatch.clone());
    Ok(Some(RustSuitesRenderSpec {
        suites: rust.suites.clone(),
        workflow_dispatch,
        default_runs_on: label.to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        mise: resolve_mise_setup(config, label)?,
        rust_version: catalog.version(PinnedTool::Rust).to_owned(),
        cargo_nextest_version: catalog.version(PinnedTool::Nextest).to_owned(),
        compile_driver,
        mbx_action,
        pull_request_cache_policy: config.workflow.pull_request_cache_policy,
        export_rustup_toolchain: catalog.exports_rustup_toolchain(),
    }))
}
