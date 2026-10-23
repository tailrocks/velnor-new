//! Orchestrator validation and emission for declared `[[stacks.rust.suites]]`.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_actionlint::PinnedActionRef;
use velnor_actions_actionlint::actions::{MR_BOXINGTON_ACTION_SHA, MR_BOXINGTON_ACTION_VERSION};
use velnor_actions_contract::{Job, RustSuiteConfig, StepRole, VelnorConfig};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_rust::CompileDriver as RustCompileDriver;
use velnor_actions_workflow_renderer::rust_suites::{
    RustSuitesRenderSpec, build_ci_suite_jobs, render_suites_workflows,
};
use velnor_actions_workflow_renderer::{CompileDriver, RenderedFile};

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
    replace_ci_suite_verifiers(&mut ci_jobs, catalog)?;
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

/// Replace only the canonical inline verifier steps in CI suite jobs with
/// their generated local composite-action calls. Standalone suite workflows
/// keep the inline form because they are rendered by a separate path.
fn replace_ci_suite_verifiers(
    jobs: &mut BTreeMap<String, Job>,
    catalog: &ToolCatalog,
) -> Result<(), OrchestratorError> {
    let restored_shell = crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
        catalog,
        crate::workflow::ToolchainPhase::Restored,
    )?;
    let installed_shell =
        crate::workflow::workflow_policy::rust_manifest_step::verifier_shell_step(
            catalog,
            crate::workflow::ToolchainPhase::Installed,
        )?;
    let restored_action = crate::workflow::verify_restored_rust_step(catalog);
    let installed_action = crate::workflow::verify_rust_manifest_step(catalog);

    for (job_id, job) in jobs {
        for step in &mut job.steps {
            if *step == restored_shell {
                *step = restored_action.clone();
            } else if *step == installed_shell {
                *step = installed_action.clone();
            } else if step.role == Some(StepRole::VerifyRustManifest) {
                return Err(OrchestratorError::Contract {
                    problem: format!("rust_suite_verifier_not_canonical:{job_id}:{}", step.name),
                });
            }
        }
    }
    Ok(())
}

/// Render standalone `.github/workflows/<slug>.yml` suite workflow files.
pub(crate) fn suite_workflow_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    validate_suites_against_repo(&prep.root, &prep.config, &prep.discovery)?;
    let catalog = &prep.emission_catalog();
    let Some(spec) = build_render_spec(&prep.config, &prep.runner_label, &prep.discovery, catalog)?
    else {
        return Ok(Vec::new());
    };
    Ok(render_suites_workflows(&spec)?)
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
        verify_restored_rust_argv: crate::workflow::verify_rust_owned_argv(
            catalog,
            crate::workflow::ToolchainPhase::Restored,
        ),
        verify_rust_argv: crate::workflow::verify_rust_owned_argv(
            catalog,
            crate::workflow::ToolchainPhase::Installed,
        ),
    }))
}

#[cfg(test)]
#[path = "rust_suites_emit_tests.rs"]
mod tests;
