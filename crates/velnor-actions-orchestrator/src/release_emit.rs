// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use velnor_actions_contract::config::{
    CrateReleaseIntent, CrateReleaseRecovery, ReleaseRegistry, RustReleaseConfig,
};
use velnor_actions_rust::release_select::DEFAULT_REGISTRY;
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::release_spec::publish_gate_condition;
use velnor_actions_workflow_renderer::release_tree::{RELEASE_HOSTED_RUNNER, render_release_files};
use velnor_actions_workflow_renderer::{MiseSetup, render::RenderedFile};

use crate::OrchestratorError;
use crate::planner::prepare::GenerationPreparation;
use crate::release::release_identity::{
    common_registry, origin_repository, plan_id_for_source, release_lock,
};
use crate::release::release_steps::{JobInputs, assemble_jobs};

#[path = "release_emit_config.rs"]
pub(super) mod config;
pub(crate) use config::{enabled_release, git_tag_release};
use config::{plz_config, release_qualification, release_render_context};

#[path = "release_emit_plan.rs"]
mod plan;
use plan::{
    approved_source, check_recovery_record, check_release_selection, release_plan,
    release_triggers, reviewed_generator_manifest,
};

#[path = "release_emit_selection.rs"]
mod selection;
use selection::tracked_manifests;
pub(crate) use selection::{release_metadata_json, select_from_json};

const RELEASE_WORKFLOW_NAME: &str = "Velnor Release";

#[path = "release_emit_validation.rs"]
mod validation;
pub use validation::validate_crate_release_workspace;

pub(crate) fn release_files(
    prep: &GenerationPreparation,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(release) = enabled_release(prep)? else {
        return Ok(Vec::new());
    };
    if release.registry == ReleaseRegistry::GitTag {
        return Ok(Vec::new());
    }
    if release.registry == ReleaseRegistry::CratesIo {
        return Err(OrchestratorError::unsupported(
            "crate_release_admission",
            "trusted publication is disabled until runtime admission and reconciliation are available",
        ));
    }
    let mise = crate::planner::pins::resolve_mise_setup(&prep.config, RELEASE_HOSTED_RUNNER)?;
    release_files_for_config(prep, &mise, release)
}

fn release_files_for_config(
    prep: &GenerationPreparation,
    mise: &MiseSetup,
    release: &RustReleaseConfig,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let ReleaseIntentInputs {
        metadata,
        known,
        selection,
        registry,
        repository,
        source_sha,
        pin,
        config,
        intent,
        source_snapshot,
        catalog,
    } = derive_release_inputs(prep, release)?;
    let source_root = source_snapshot
        .as_ref()
        .map_or(prep.root.as_path(), |source| source.root());
    let plan_id = plan_id_for_source(&source_sha);
    let version = release
        .recovery
        .as_ref()
        .map(|record| record.version.as_str());
    let mut plan = release_plan(
        &selection,
        &repository,
        &source_sha,
        &plan_id,
        &registry,
        version,
        release.exact_source_sha().is_some(),
    );
    if intent.oidc_qualification {
        plan.qualification_dispatch =
            velnor_actions_workflow_renderer::release_spec::QualificationDispatch::Enabled;
    }
    let jobs = build_release_jobs(
        prep,
        mise,
        release,
        &registry,
        &repository,
        &plan,
        ((&pin.0, &pin.1), &catalog),
    )?;
    let concurrency = release_lock(
        &metadata,
        source_root,
        &release.manifest_path,
        &known,
        &registry,
        &repository,
    )?;
    let spec = ReleaseWorkflowSpec {
        name: RELEASE_WORKFLOW_NAME.to_owned(),
        repository,
        default_branch: prep.default_branch.clone(),
        triggers: release_triggers(&prep.default_branch, &plan),
        concurrency,
        jobs,
        plan,
        publish_environment: release.environment.clone(),
        bootstrap_environment: release.environment.clone(),
        journal_predecessor: release.journal_predecessor.clone(),
        new_crates: release.new_crates.clone(),
        qualification: release_qualification(&intent),
        release_plz_version: catalog
            .version(velnor_actions_mise::PinnedTool::ReleasePlz)
            .to_owned(),
    };
    let ctx = release_render_context();
    Ok(render_release_files(&spec, &ctx, &config, &intent)?.as_sorted_vec())
}

fn build_release_jobs(
    prep: &GenerationPreparation,
    mise: &MiseSetup,
    release: &RustReleaseConfig,
    registry: &str,
    repository: &str,
    plan: &velnor_actions_workflow_renderer::release_spec::ReleasePlan,
    inputs: ((&str, &str), &velnor_actions_mise::ToolCatalog),
) -> Result<
    std::collections::BTreeMap<
        String,
        velnor_actions_workflow_renderer::release_jobs::ReleaseJobSpec,
    >,
    OrchestratorError,
> {
    let gate = publish_gate_condition(repository, &prep.default_branch, plan);
    let preparation_gate = format!(
        "github.repository == '{repository}' && github.event_name == 'push' && github.ref == 'refs/heads/{}' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'",
        prep.default_branch
    );
    let (generator, source_catalog) = inputs;
    let catalog = source_catalog.clone().with_export_rustup_toolchain(false);
    let registry_arg = (registry != DEFAULT_REGISTRY).then_some(registry);
    let generator_version = env!("CARGO_PKG_VERSION");
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(
        generator_version,
        RELEASE_HOSTED_RUNNER,
        generator.0,
        generator.1,
    )?;
    let mut jobs = assemble_jobs(&JobInputs {
        release,
        gate,
        preparation_gate,
        registry: registry_arg,
        label: RELEASE_HOSTED_RUNNER,
        mise,
        catalog: &catalog,
        acquire: &acquire,
        generator_version,
        generator_source_sha: generator.0,
        generator_manifest_sha256: generator.1,
    })?;
    if let Some(preflight) = jobs.get_mut("release-preflight") {
        preflight.condition =
            velnor_actions_workflow_renderer::release_spec::preflight_gate_condition(
                repository,
                &prep.default_branch,
                plan,
            );
    }
    Ok(jobs)
}

struct IntentSettings<'a> {
    default_branch: &'a str,
    catalog: &'a velnor_actions_mise::ToolCatalog,
    generator_version: &'a str,
}

fn crate_release_intent(
    settings: &IntentSettings<'_>,
    release: &RustReleaseConfig,
    repository: &str,
    registry: &str,
    generator_pin: (&str, &str),
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    pr_write_paths: Vec<String>,
) -> CrateReleaseIntent {
    CrateReleaseIntent {
        schema: 1,
        oidc_qualification: release.oidc_qualification.is_enabled(),
        repository: repository.to_owned(),
        default_branch: settings.default_branch.to_owned(),
        environment: release.environment.clone(),
        required_workflow_path: release.required_workflow_path.clone(),
        trusted_reviewer_id: release.trusted_reviewer_id,
        rust_version: settings
            .catalog
            .version(velnor_actions_mise::PinnedTool::Rust)
            .to_owned(),
        generator_version: settings.generator_version.to_owned(),
        generator_source_sha: generator_pin.0.to_owned(),
        generator_manifest_sha256: generator_pin.1.to_owned(),
        release_plz_version: settings
            .catalog
            .version(velnor_actions_mise::PinnedTool::ReleasePlz)
            .to_owned(),
        runner: RELEASE_HOSTED_RUNNER.to_owned(),
        authentication: release.effective_authentication(),
        manifest_path: release.manifest_path.clone(),
        pr_write_paths,
        registry: registry.to_owned(),
        packages: config
            .packages
            .iter()
            .map(|package| package.name.clone())
            .collect(),
        publish_features: config
            .packages
            .iter()
            .map(|package| (package.name.clone(), package.publish_features.clone()))
            .collect(),
        version_groups: release.version_groups.clone(),
        new_crates: release.new_crates.clone(),
        tag_template: release.tag_name.clone(),
        journal_predecessor: release.journal_predecessor.clone(),
        continuation_source_sha: release.continuation_source_sha.clone(),
        recovery: release
            .recovery
            .as_ref()
            .map(|recovery| CrateReleaseRecovery {
                package: recovery.package.clone(),
                version: recovery.version.clone(),
                source_sha: recovery.source_sha.clone(),
            }),
    }
}

#[path = "release_emit_snapshot.rs"]
mod snapshot;
pub(crate) use snapshot::derive_intent_from_snapshot;

struct ReleaseIntentInputs {
    metadata: String,
    known: BTreeSet<String>,
    selection: velnor_actions_rust::release_select::ReleaseSelection,
    registry: String,
    repository: String,
    source_sha: String,
    pin: (String, String),
    config: velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    intent: CrateReleaseIntent,
    source_snapshot: Option<crate::crate_release_qualification_snapshot::SourceSnapshot>,
    catalog: velnor_actions_mise::ToolCatalog,
}

fn derive_release_inputs(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
) -> Result<ReleaseIntentInputs, OrchestratorError> {
    let source_sha = approved_source(&prep.root, release)?;
    let source_snapshot = if release.exact_source_sha().is_some() {
        let git = crate::crate_release_git::ReleaseGit::system().map_err(|_| {
            OrchestratorError::Contract {
                problem: "release_source_git_unavailable".to_owned(),
            }
        })?;
        Some(
            crate::crate_release_qualification_snapshot::snapshot_git_objects(
                &prep.root,
                &source_sha,
                &git,
            )?,
        )
    } else {
        None
    };
    let source_root = source_snapshot
        .as_ref()
        .map_or(prep.root.as_path(), |source| source.root());
    let catalog = if let Some(source) = &source_snapshot {
        crate::tool_catalog_resolve::resolve_snapshot_catalog(
            source.root(),
            &prep.config,
            source.nested_toolchains(),
        )?
    } else {
        prep.catalog.clone()
    };
    let metadata = selection::release_metadata_json_at(
        prep,
        source_root,
        &catalog,
        release.manifest_path.as_str(),
    )?;
    let known = if let Some(source) = &source_snapshot {
        source.manifests().clone()
    } else {
        tracked_manifests(source_root)?
    };
    let snapshot::SourceInputs {
        selection,
        registry,
        config,
        paths,
    } = snapshot::derive_source_inputs(source_root, release, &metadata, &known)?;
    let repository = origin_repository(&prep.root)?;
    let pin = plan::reviewed_current_generator_manifest(
        prep.discovery.consumer_manifest_json.as_deref(),
        env!("CARGO_PKG_VERSION"),
    )?;
    let intent = crate_release_intent(
        &IntentSettings {
            default_branch: &prep.default_branch,
            catalog: &catalog,
            generator_version: env!("CARGO_PKG_VERSION"),
        },
        release,
        &repository,
        &registry,
        (&pin.0, &pin.1),
        &config,
        paths,
    );
    Ok(ReleaseIntentInputs {
        metadata,
        known,
        selection,
        registry,
        repository,
        source_sha,
        pin,
        config,
        intent,
        source_snapshot,
        catalog,
    })
}

fn reviewed_pr_write_paths(
    root: &std::path::Path,
    release: &RustReleaseConfig,
    metadata: &str,
    known: &BTreeSet<String>,
) -> Result<Vec<String>, OrchestratorError> {
    let record = velnor_actions_rust::metadata::parse_metadata_json(
        metadata,
        root,
        &release.manifest_path,
        known,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    Ok(release_pr_write_paths(&record))
}

#[path = "release_emit_write_paths.rs"]
mod write_paths;
use write_paths::release_pr_write_paths;

#[cfg(test)]
#[path = "release_emit_tests.rs"]
mod tests;
