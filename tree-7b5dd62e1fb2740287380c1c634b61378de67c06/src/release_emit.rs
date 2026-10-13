//! Consumer release emission: selection plus release-tree assembly.
//!
//! Builds the release family (`release.yml` plus both effective
//! configs) when `[stacks.rust.release].enabled` under `consumer-v1`;
//! anything else yields no files. Selection reuses the Rust adapter's
//! allowlist resolution over pinned `cargo metadata`; identities the
//! config schema does not carry derive in
//! [`crate::release_identity`], and jobs assemble in
//! [`crate::release_steps`]. Publication-graph validation against live
//! registry state stays a runtime preflight concern: generation runs
//! no registry queries.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_config::config::{ReleaseAuthentication, RustReleaseConfig};
use velnor_actions_mise::{MetadataDiscovery, ToolCatalog};
use velnor_actions_rust_core::release_select::{
    DEFAULT_REGISTRY, ReleaseRequest as SelectRequest, ReleaseScope, ReleaseSelection,
    select_release_set,
};
use velnor_actions_workflow_release::release_config::{
    BootstrapReleasePlzConfig, ReleasePlzConfig, ReleasePlzPackage,
};
use velnor_actions_workflow_release::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_release::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseTriggers, lock::stable_lock_group, publish_gate_condition,
    validate_source_sha,
};
use velnor_actions_workflow_release::release_tree::{ReleaseRenderContext, render_release_files};
use velnor_actions_workflow_steps::MiseSetup;
use velnor_actions_workflow_tree::rendered::RenderedFile;

use crate::prepare::GenerationPreparation;
use crate::release_identity::{
    common_registry, head_sha, origin_repository, plan_id_for_source, workspace_slug,
};
use crate::release_steps::{JobInputs, assemble_jobs};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::config::CONFIG_REL;

/// Release workflow display name (fixed; config carries no workflow name).
const RELEASE_WORKFLOW_NAME: &str = "Velnor Release";

/// Render the release family, or nothing when release is not enabled.
///
/// # Errors
///
/// Returns config errors for policy or bootstrap-record conflicts,
/// preparation errors when release metadata cannot run, contract
/// errors for unresolvable identities or selections, unsupported
/// errors for native version groups, and render errors for invalid
/// assembled specs.
pub(crate) fn release_files(
    prep: &GenerationPreparation,
    mise: &MiseSetup,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(release) = enabled_release(prep)? else {
        return Ok(Vec::new());
    };
    if !release.version_groups.is_empty() {
        return Err(OrchestratorError::unsupported(
            "version_groups",
            "native groups need renderer config support: remove version_groups",
        ));
    }
    let selection = select_packages(prep, release)?;
    if selection.is_empty() {
        return Err(OrchestratorError::Contract {
            problem: "release_nothing_selected".to_owned(),
        });
    }
    check_bootstrap_record(release, &selection)?;
    let registry = common_registry(&selection)?;
    let repository = origin_repository(&prep.root)?;
    let source_sha = approved_source(&prep.root, release)?;
    let plan_id = plan_id_for_source(&source_sha);
    let version = release
        .bootstrap
        .as_ref()
        .map(|record| record.version.as_str());
    let bootstrap = bootstrap_plan(
        &selection,
        &repository,
        &source_sha,
        &plan_id,
        &registry,
        version,
    );
    let gate = publish_gate_condition(&repository, &bootstrap);
    let catalog = ToolCatalog::pinned();
    // Omit `--registry` for the cargo-implicit default: release-plz 0.3.169
    // resolves the flag value from Cargo config, where that name is absent.
    let registry_arg = (registry != DEFAULT_REGISTRY).then_some(registry.as_str());
    let jobs = assemble_jobs(&JobInputs {
        release,
        gate,
        sha: &source_sha,
        registry: registry_arg,
        label: &prep.runner_label,
        mise,
        catalog: &catalog,
    })?;
    let concurrency = stable_lock_group(
        &registry,
        &repository,
        &workspace_slug(&release.manifest_path),
    )?;
    let spec = ReleaseWorkflowSpec {
        name: RELEASE_WORKFLOW_NAME.to_owned(),
        repository,
        triggers: release_triggers(&prep.default_branch, &bootstrap),
        concurrency,
        jobs,
        bootstrap,
        publish_environment: release.environment.clone(),
        bootstrap_environment: release.environment.clone(),
    };
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: prep.runner_label.clone(),
    };
    let config = plz_config(release, &selection);
    let bootstrap_config = BootstrapReleasePlzConfig::new(config.clone())?;
    Ok(render_release_files(&spec, &ctx, &config, &bootstrap_config)?.as_sorted_vec())
}

/// Borrow the active release section, enforcing the consumer-only policy.
///
/// # Errors
///
/// Returns a config error when `velnor-repository-v1` enables release:
/// the filename keeps its Velnor-internal meaning there, so consumer
/// release content must not be emitted and the explicit opt-in must
/// not be silently ignored.
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

/// Resolve the release set over pinned `cargo metadata`.
///
/// # Errors
///
/// Returns preparation errors when metadata cannot run and contract
/// errors for unknown, forbidden, or unsupported selections.
fn select_packages(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
) -> Result<ReleaseSelection, OrchestratorError> {
    let manifest = release.manifest_path.as_str();
    let json = release_metadata_json(&prep.root, manifest)?;
    let scope = if release.publishable_workspace {
        ReleaseScope::PublishableWorkspace
    } else {
        ReleaseScope::Packages(release.packages.clone())
    };
    let affected = BTreeSet::new();
    let supported: &[String] = &[];
    let request = SelectRequest {
        metadata_json: &json,
        repo_root: &prep.root,
        manifest_hint: manifest,
        scope: &scope,
        enabled: true,
        affected: &affected,
        supported_registries: supported,
    };
    select_release_set(&request).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

/// Run pinned metadata discovery for the release manifest.
///
/// # Errors
///
/// Returns [`OrchestratorError::PreparationIncomplete`] when the tool
/// cannot run or the manifest read fails.
fn release_metadata_json(
    root: &std::path::Path,
    manifest: &str,
) -> Result<String, OrchestratorError> {
    let incomplete = |problem: String| OrchestratorError::PreparationIncomplete {
        manifest: manifest.to_owned(),
        problem,
    };
    let catalog = ToolCatalog::pinned();
    let request =
        MetadataDiscovery::new(root.join(manifest)).map_err(|err| incomplete(err.to_string()))?;
    let command = request
        .command(&catalog)
        .map_err(|err| incomplete(err.to_string()))?;
    let output = command.run().map_err(|err| incomplete(err.to_string()))?;
    if !output.success {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let first: String = stderr
            .lines()
            .next()
            .unwrap_or("metadata_failed")
            .chars()
            .take(160)
            .collect();
        return Err(incomplete(format!("metadata_failed:{first}")));
    }
    output
        .stdout_text("mise")
        .map_err(|err| incomplete(err.to_string()))
}

/// Cross-check the bootstrap record against the selected set.
///
/// # Errors
///
/// Returns config errors when the record names an unselected package
/// or a version disagreeing with workspace metadata.
fn check_bootstrap_record(
    release: &RustReleaseConfig,
    selection: &ReleaseSelection,
) -> Result<(), OrchestratorError> {
    if release.authentication != ReleaseAuthentication::BootstrapToken {
        return Ok(());
    }
    let Some(record) = release.bootstrap.as_ref() else {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.bootstrap",
            "missing_bootstrap_record",
        ));
    };
    let selected = selection
        .packages
        .iter()
        .find(|package| package.name == record.package);
    let Some(selected) = selected else {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.bootstrap.package",
            "bootstrap_package_not_selected",
        ));
    };
    if selected.version != record.version {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.bootstrap.version",
            format!("bootstrap_version_mismatch:{}", selected.version),
        ));
    }
    Ok(())
}

/// Approved source: configured SHA for bootstrap, `HEAD` for routine.
///
/// # Errors
///
/// Returns config errors for a missing record and contract errors for
/// an unresolvable `HEAD` or malformed SHA.
fn approved_source(
    root: &std::path::Path,
    release: &RustReleaseConfig,
) -> Result<String, OrchestratorError> {
    if release.authentication == ReleaseAuthentication::BootstrapToken {
        let Some(record) = release.bootstrap.as_ref() else {
            return Err(OrchestratorError::config(
                CONFIG_REL,
                "stacks.rust.release.bootstrap",
                "missing_bootstrap_record",
            ));
        };
        validate_source_sha(&record.source_sha)?;
        return Ok(record.source_sha.clone());
    }
    head_sha(root)
}

/// Assemble the approved exact-source plan from selection plus identity.
///
/// `version` is the configured bootstrap version, present only in
/// bootstrap-token mode (config validation forbids the record under
/// trusted publishing); routine publishers resolve versions at runtime.
fn bootstrap_plan(
    selection: &ReleaseSelection,
    repository: &str,
    sha: &str,
    plan_id: &str,
    registry: &str,
    version: Option<&str>,
) -> BootstrapPlan {
    let packages: BTreeMap<String, String> = selection
        .packages
        .iter()
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect();
    BootstrapPlan {
        plan_id: plan_id.to_owned(),
        repository: repository.to_owned(),
        source_sha: sha.to_owned(),
        registry: registry.to_owned(),
        packages,
        version: version.map(str::to_owned),
    }
}

/// Trusted-branch push plus plan-bound dispatch (no schedule: the config
/// schema carries no cron field, so emission invents none).
fn release_triggers(branch: &str, bootstrap: &BootstrapPlan) -> ReleaseTriggers {
    let mut dispatch_inputs = vec![
        DispatchInput {
            name: "plan".to_owned(),
            description: "Approved release plan".to_owned(),
            required: true,
            default: Some(bootstrap.plan_id.clone()),
        },
        DispatchInput {
            name: "source_sha".to_owned(),
            description: "Approved source SHA".to_owned(),
            required: true,
            default: Some(bootstrap.source_sha.clone()),
        },
    ];
    if let Some(version) = &bootstrap.version {
        dispatch_inputs.push(DispatchInput {
            name: "version".to_owned(),
            description: "Approved crate version".to_owned(),
            required: true,
            default: Some(version.clone()),
        });
    }
    ReleaseTriggers {
        push_branches: vec![branch.to_owned()],
        schedule: None,
        dispatch_inputs,
    }
}

/// Effective normal-policy config: tag plus allowlist, no features.
///
/// The schema carries no `publish_features`, so emission sets none;
/// `semver_check` stays true per the release contract.
fn plz_config(release: &RustReleaseConfig, selection: &ReleaseSelection) -> ReleasePlzConfig {
    ReleasePlzConfig {
        tag_pattern: release.tag_name.clone(),
        semver_check: true,
        packages: selection
            .packages
            .iter()
            .map(|package| ReleasePlzPackage {
                name: package.name.clone(),
                publish_features: Vec::new(),
            })
            .collect(),
    }
}
