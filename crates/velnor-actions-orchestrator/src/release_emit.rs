// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{
    CrateReleaseIntent, CrateReleaseRecovery, ReleaseAuthentication, ReleaseRegistry,
    RustReleaseConfig,
};
use velnor_actions_rust::release_select::DEFAULT_REGISTRY;
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::release_spec::publish_gate_condition;
use velnor_actions_workflow_renderer::release_tree::{
    RELEASE_HOSTED_RUNNER, ReleaseRenderContext, render_release_files,
};
use velnor_actions_workflow_renderer::{MiseSetup, render::RenderedFile};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::prepare::GenerationPreparation;
use crate::release_identity::{
    common_registry, origin_repository, plan_id_for_source, workspace_lock,
};
use crate::release_steps::{JobInputs, assemble_jobs};

#[path = "release_emit_config.rs"]
mod config;
use config::plz_config;

#[path = "release_emit_plan.rs"]
mod plan;
use plan::{
    approved_source, check_creation_selection, check_recovery_record, release_plan,
    release_triggers, reviewed_generator_manifest,
};

#[path = "release_emit_selection.rs"]
mod selection;
pub(crate) use selection::{release_metadata_json, select_from_json};
use selection::{select_from_json_with_known, tracked_manifests};

const RELEASE_WORKFLOW_NAME: &str = "Velnor Release";

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
    if !release.version_groups.is_empty() {
        return Err(OrchestratorError::unsupported(
            "version_groups",
            "native groups need renderer config support: remove version_groups",
        ));
    }
    let mise = crate::pins::resolve_mise_setup(&prep.config, RELEASE_HOSTED_RUNNER)?;
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
    } = derive_release_inputs(prep, release)?;
    let plan_id = plan_id_for_source(&source_sha);
    let version = release
        .recovery
        .as_ref()
        .map(|record| record.version.as_str());
    let plan = release_plan(
        &selection,
        &repository,
        &source_sha,
        &plan_id,
        &registry,
        version,
        release.recovery.is_some() || release.new_crates.is_some(),
    );
    let jobs = build_release_jobs(
        prep,
        mise,
        release,
        &registry,
        &repository,
        &plan,
        (&pin.0, &pin.1),
    )?;
    let concurrency = workspace_lock(
        &metadata,
        &prep.root,
        &release.manifest_path,
        &known,
        &registry,
        &repository,
    )?;
    let spec = ReleaseWorkflowSpec {
        name: RELEASE_WORKFLOW_NAME.to_owned(),
        repository,
        triggers: release_triggers(&prep.default_branch, &plan, release.new_crates.is_some()),
        concurrency,
        jobs,
        plan,
        publish_environment: release.environment.clone(),
        bootstrap_environment: release.environment.clone(),
        journal_predecessor: release.journal_predecessor.clone(),
        new_crates: release.new_crates.clone(),
        release_plz_version: prep
            .catalog
            .version(velnor_actions_mise::PinnedTool::ReleasePlz)
            .to_owned(),
    };
    let ctx = release_render_context();
    Ok(render_release_files(&spec, &ctx, &config, &intent)?.as_sorted_vec())
}

fn release_render_context() -> ReleaseRenderContext {
    ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: RELEASE_HOSTED_RUNNER.to_owned(),
    }
}

fn build_release_jobs(
    prep: &GenerationPreparation,
    mise: &MiseSetup,
    release: &RustReleaseConfig,
    registry: &str,
    repository: &str,
    plan: &velnor_actions_workflow_renderer::release_spec::ReleasePlan,
    generator: (&str, &str),
) -> Result<
    std::collections::BTreeMap<
        String,
        velnor_actions_workflow_renderer::release_jobs::ReleaseJobSpec,
    >,
    OrchestratorError,
> {
    let gate = publish_gate_condition(
        repository,
        &prep.default_branch,
        plan,
        release.effective_authentication() == ReleaseAuthentication::TrustedPublishing,
    );
    let preparation_gate = format!(
        "github.repository == '{repository}' && github.event_name == 'push' && github.ref == 'refs/heads/{}' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'",
        prep.default_branch
    );
    let catalog = prep.catalog.clone().with_export_rustup_toolchain(false);
    let registry_arg = (registry != DEFAULT_REGISTRY).then_some(registry);
    let generator_version = env!("CARGO_PKG_VERSION");
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(
        generator_version,
        RELEASE_HOSTED_RUNNER,
        generator.0,
        generator.1,
    )?;
    assemble_jobs(&JobInputs {
        release,
        gate,
        preparation_gate,
        registry: registry_arg,
        label: RELEASE_HOSTED_RUNNER,
        mise,
        catalog: &catalog,
        acquire: &acquire,
        generator_version,
    })
}

struct IntentSettings<'a> {
    default_branch: &'a str,
    catalog: &'a velnor_actions_mise::ToolCatalog,
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
        repository: repository.to_owned(),
        default_branch: settings.default_branch.to_owned(),
        environment: release.environment.clone(),
        required_workflow_path: release.required_workflow_path.clone(),
        trusted_reviewer_id: release.trusted_reviewer_id,
        rust_version: settings
            .catalog
            .version(velnor_actions_mise::PinnedTool::Rust)
            .to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
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
        new_crates: release.new_crates.clone(),
        tag_template: release.tag_name.clone(),
        journal_predecessor: release.journal_predecessor.clone(),
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
}

fn derive_release_inputs(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
) -> Result<ReleaseIntentInputs, OrchestratorError> {
    let metadata = release_metadata_json(prep, release.manifest_path.as_str())?;
    let known = tracked_manifests(&prep.root)?;
    let selection = select_from_json_with_known(prep, release, &metadata, &known)?;
    if selection.is_empty() {
        return Err(OrchestratorError::Contract {
            problem: "release_nothing_selected".to_owned(),
        });
    }
    check_recovery_record(release, &selection)?;
    check_creation_selection(release, &selection)?;
    let registry = common_registry(&selection)?;
    let repository = origin_repository(&prep.root)?;
    let source_sha = approved_source(&prep.root, release)?;
    let pin = reviewed_generator_manifest(
        prep.discovery.consumer_manifest_json.as_deref(),
        prep.discovery.consumer_manifest_stand_in,
        env!("CARGO_PKG_VERSION"),
    )?;
    let config = plz_config(release, &selection)?;
    let paths = reviewed_pr_write_paths(&prep.root, release, &metadata, &known)?;
    let intent = crate_release_intent(
        &IntentSettings {
            default_branch: &prep.default_branch,
            catalog: &prep.catalog,
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

fn release_pr_write_paths(record: &velnor_actions_rust::metadata::WorkspaceRecord) -> Vec<String> {
    let join = |dir: &str, file: &str| {
        if dir.is_empty() {
            file.to_owned()
        } else {
            format!("{dir}/{file}")
        }
    };
    let mut paths = BTreeSet::new();
    for name in ["Cargo.toml", "Cargo.lock", "CHANGELOG.md"] {
        paths.insert(join(&record.workspace_root, name));
    }
    for package in record
        .packages
        .iter()
        .filter(|package| package.in_workspace && !package.external)
    {
        paths.insert(package.manifest.clone());
        let dir = package.manifest.rsplit_once('/').map_or("", |(dir, _)| dir);
        paths.insert(join(dir, "CHANGELOG.md"));
    }
    paths.into_iter().collect()
}

pub(crate) fn enabled_release(
    prep: &GenerationPreparation,
) -> Result<Option<&RustReleaseConfig>, OrchestratorError> {
    let release = prep.config.stacks.rust.as_ref().map(|stack| &stack.release);
    let Some(release) = release else {
        return Ok(None);
    };
    if !release.enabled {
        return Ok(None);
    }
    if prep.config.workflow.policy != WorkflowPolicy::ConsumerV1 {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.enabled",
            "release_requires_consumer_policy",
        ));
    }
    Ok(Some(release))
}

pub(crate) fn git_tag_release(
    prep: &GenerationPreparation,
) -> Result<Option<&RustReleaseConfig>, OrchestratorError> {
    if prep
        .config
        .stacks
        .rust
        .as_ref()
        .is_some_and(|rust| rust.binary_release.stable.enabled)
    {
        return Ok(None);
    }
    Ok(enabled_release(prep)?.filter(|release| release.registry == ReleaseRegistry::GitTag))
}

#[cfg(test)]
#[path = "release_emit_tests.rs"]
mod tests;
