use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{ReleaseAuthentication, ReleaseRegistry, RustReleaseConfig};
use velnor_actions_mise::{MetadataDiscovery, ToolCatalog};
use velnor_actions_rust::release_select::{
    DEFAULT_REGISTRY, ReleaseRequest as SelectRequest, ReleaseScope, ReleaseSelection,
    select_release_set,
};
use velnor_actions_workflow_renderer::release_config::BootstrapReleasePlzConfig;
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::release_spec::{
    BootstrapPlan, DispatchInput, ReleaseTriggers, lock::stable_lock_group, publish_gate_condition,
    validate_source_sha,
};
use velnor_actions_workflow_renderer::release_tree::{ReleaseRenderContext, render_release_files};
use velnor_actions_workflow_renderer::{MiseSetup, render::RenderedFile};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::prepare::GenerationPreparation;
use crate::release_identity::{
    common_registry, head_sha, origin_repository, plan_id_for_source, workspace_slug,
};
use crate::release_steps::{JobInputs, assemble_jobs};

#[path = "release_emit_config.rs"]
mod config;
use config::plz_config;

const RELEASE_WORKFLOW_NAME: &str = "Velnor Release";

pub(crate) fn release_files(
    prep: &GenerationPreparation,
    mise: &MiseSetup,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let Some(release) = enabled_release(prep)? else {
        return Ok(Vec::new());
    };
    if release.registry == ReleaseRegistry::GitTag {
        return Ok(Vec::new());
    }
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
    let catalog = &prep.emission_catalog();
    let registry_arg = (registry != DEFAULT_REGISTRY).then_some(registry.as_str());
    let jobs = assemble_jobs(&JobInputs {
        release,
        gate,
        sha: &source_sha,
        registry: registry_arg,
        label: &prep.runner_label,
        mise,
        catalog,
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

fn select_packages(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
) -> Result<ReleaseSelection, OrchestratorError> {
    let json = release_metadata_json(&prep.root, release.manifest_path.as_str(), &prep.catalog)?;
    select_from_json(prep, release, &json)
}

pub(crate) fn select_from_json(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
    json: &str,
) -> Result<ReleaseSelection, OrchestratorError> {
    let manifest = release.manifest_path.as_str();
    let scope = if release.publishable_workspace {
        ReleaseScope::PublishableWorkspace
    } else if release.registry == ReleaseRegistry::GitTag {
        ReleaseScope::GitTagPackages(release.packages.clone())
    } else {
        ReleaseScope::Packages(release.packages.clone())
    };
    let affected = BTreeSet::new();
    let supported: &[String] = &[];
    let request = SelectRequest {
        metadata_json: json,
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

pub(crate) fn release_metadata_json(
    root: &std::path::Path,
    manifest: &str,
    catalog: &ToolCatalog,
) -> Result<String, OrchestratorError> {
    let incomplete = |problem: String| OrchestratorError::PreparationIncomplete {
        manifest: manifest.to_owned(),
        problem,
    };
    crate::inventory::require_rust_pin(catalog).map_err(|err| incomplete(err.to_string()))?;
    let request =
        MetadataDiscovery::new(root.join(manifest)).map_err(|err| incomplete(err.to_string()))?;
    let command = request
        .command(catalog)
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

fn check_bootstrap_record(
    release: &RustReleaseConfig,
    selection: &ReleaseSelection,
) -> Result<(), OrchestratorError> {
    if release.effective_authentication() != ReleaseAuthentication::BootstrapToken {
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

fn approved_source(
    root: &std::path::Path,
    release: &RustReleaseConfig,
) -> Result<String, OrchestratorError> {
    if release.effective_authentication() == ReleaseAuthentication::BootstrapToken {
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
