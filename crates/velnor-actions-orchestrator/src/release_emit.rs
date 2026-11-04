// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{CrateReleaseIntent, CrateReleaseRecovery, ReleaseAuthentication, ReleaseRegistry, RustReleaseConfig};
use velnor_actions_mise::{GitRequest, MetadataDiscovery};
use velnor_actions_rust::release_select::{
    DEFAULT_REGISTRY, ReleaseRequest as SelectRequest, ReleaseScope, ReleaseSelection,
    select_release_set,
};
use velnor_actions_workflow_renderer::release_jobs::ReleaseWorkflowSpec;
use velnor_actions_workflow_renderer::release_spec::{
    DispatchInput, ReleasePlan, ReleaseTriggers, publish_gate_condition,
    validate_source_sha,
};
use velnor_actions_workflow_renderer::release_tree::{RELEASE_HOSTED_RUNNER, ReleaseRenderContext, render_release_files};
use velnor_actions_workflow_renderer::{MiseSetup, render::RenderedFile};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::prepare::GenerationPreparation;
use crate::release_identity::{
    common_registry, head_sha, origin_repository, plan_id_for_source, workspace_lock,
};
use crate::release_steps::{JobInputs, assemble_jobs};

#[path = "release_emit_config.rs"]
mod config;
use config::plz_config;

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
    let metadata = release_metadata_json(prep, release.manifest_path.as_str())?;
    let known_manifests = tracked_manifests(&prep.root)?;
    let selection = select_from_json_with_known(prep, release, &metadata, &known_manifests)?;
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
    let gate = publish_gate_condition(
        &repository,
        &prep.default_branch,
        &plan,
        release.effective_authentication() == ReleaseAuthentication::TrustedPublishing,
    );
    let preparation_gate = format!(
        "github.repository == '{repository}' && github.event_name == 'push' && github.ref == 'refs/heads/{}'",
        prep.default_branch
    );
    let catalog = prep.catalog.clone().with_export_rustup_toolchain(false);
    let registry_arg = (registry != DEFAULT_REGISTRY).then_some(registry.as_str());
    let generator_version = env!("CARGO_PKG_VERSION");
    let (generator_source_sha, generator_manifest_sha256) = reviewed_generator_manifest(prep.discovery.consumer_manifest_json.as_deref(), prep.discovery.consumer_manifest_stand_in, generator_version)?;
    let acquire =
        velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(generator_version, RELEASE_HOSTED_RUNNER, &generator_source_sha, &generator_manifest_sha256)?;
    let jobs = assemble_jobs(&JobInputs {
        release,
        gate,
        preparation_gate,
        registry: registry_arg,
        label: RELEASE_HOSTED_RUNNER,
        mise,
        catalog: &catalog,
        acquire: &acquire,
        generator_version,
    })?;
    let concurrency = workspace_lock(&metadata, &prep.root, &release.manifest_path, &known_manifests, &registry, &repository)?;
    let config = plz_config(release, &selection)?;
    let record = velnor_actions_rust::metadata::parse_metadata_json(&metadata, &prep.root, &release.manifest_path, &known_manifests)
        .map_err(|err| OrchestratorError::Contract { problem: err.to_string() })?;
    let pr_write_paths = release_pr_write_paths(&record);
    let intent = crate_release_intent(prep, release, &repository, &registry, &generator_source_sha, &generator_manifest_sha256, &config, pr_write_paths);
    let spec = ReleaseWorkflowSpec {
        name: RELEASE_WORKFLOW_NAME.to_owned(),
        repository,
        triggers: release_triggers(&prep.default_branch, &plan),
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
    let ctx = ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        runs_on: RELEASE_HOSTED_RUNNER.to_owned(),
    };
    Ok(render_release_files(&spec, &ctx, &config, &intent)?.as_sorted_vec())
}

fn crate_release_intent(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
    repository: &str,
    registry: &str,
    generator_source_sha: &str,
    generator_manifest_sha256: &str,
    config: &velnor_actions_workflow_renderer::release_config::ReleasePlzConfig,
    pr_write_paths: Vec<String>,
) -> CrateReleaseIntent {
    CrateReleaseIntent {
        schema: 1,
        repository: repository.to_owned(),
        default_branch: prep.default_branch.clone(),
        environment: release.environment.clone(),
        required_workflow_path: release.required_workflow_path.clone(),
        trusted_reviewer_id: release.trusted_reviewer_id,
        rust_version: prep
            .catalog
            .version(velnor_actions_mise::PinnedTool::Rust)
            .to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        generator_source_sha: generator_source_sha.to_owned(),
        generator_manifest_sha256: generator_manifest_sha256.to_owned(),
        release_plz_version: prep.catalog.version(velnor_actions_mise::PinnedTool::ReleasePlz).to_owned(),
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
        publish_features: config.packages.iter().map(|package| (package.name.clone(), package.publish_features.clone())).collect(),
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

fn release_pr_write_paths(record: &velnor_actions_rust::metadata::WorkspaceRecord) -> Vec<String> {
    let join = |dir: &str, file: &str| if dir.is_empty() { file.to_owned() } else { format!("{dir}/{file}") };
    let mut paths = BTreeSet::new();
    for name in ["Cargo.toml", "Cargo.lock", "CHANGELOG.md"] {
        paths.insert(join(&record.workspace_root, name));
    }
    for package in record.packages.iter().filter(|package| package.in_workspace && !package.external) {
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

pub(crate) fn select_from_json(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
    json: &str,
) -> Result<ReleaseSelection, OrchestratorError> {
    let known_manifests = tracked_manifests(&prep.root)?;
    select_from_json_with_known(prep, release, json, &known_manifests)
}

fn select_from_json_with_known(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
    json: &str,
    known_manifests: &BTreeSet<String>,
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
        known_manifests,
        supported_registries: supported,
    };
    let record = velnor_actions_rust::metadata::parse_metadata_json(
        json,
        &prep.root,
        manifest,
        known_manifests,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    let workspace_manifest = if record.workspace_root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{}/Cargo.toml", record.workspace_root)
    };
    if !velnor_actions_rust::tracked_manifest_in_source(&prep.root, known_manifests, manifest)
        || !velnor_actions_rust::tracked_manifest_in_source(&prep.root, known_manifests, &workspace_manifest)
        || record.packages.iter().any(|package| {
            !package.external
                && !velnor_actions_rust::tracked_manifest_in_source(&prep.root, known_manifests, &package.manifest)
        })
    {
        return Err(OrchestratorError::Contract {
            problem: "release_manifest_not_tracked".to_owned(),
        });
    }
    select_release_set(&request).map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })
}

fn tracked_manifests(root: &std::path::Path) -> Result<BTreeSet<String>, OrchestratorError> {
    let failure = || OrchestratorError::Contract {
        problem: "release_tracked_manifests_unavailable".to_owned(),
    };
    let output = GitRequest::ls_files(vec!["-z".into()])
        .run_in(root)
        .map_err(|_| failure())?;
    if !output.success {
        return Err(failure());
    }
    let paths: BTreeSet<String> =
        crate::git_paths::split_nul_paths(&output.stdout).map_err(|_| failure())?;
    Ok(paths
        .into_iter()
        .filter(|path| {
            std::path::Path::new(path)
                .file_name()
                .is_some_and(|name| name == "Cargo.toml")
        })
        .collect())
}

pub(crate) fn release_metadata_json(
    prep: &GenerationPreparation,
    manifest: &str,
) -> Result<String, OrchestratorError> {
    let catalog = &prep.catalog;
    if let Some(guard) = prep.operator_toolchain_guard() {
        crate::inventory::require_metadata_tools(catalog, Some(guard))?;
        return MetadataDiscovery::new(prep.root.join(manifest))
            .and_then(|request| request.run_with_operator_guard(catalog, guard))
            .map_err(|_| OrchestratorError::ToolchainGuard {
                problem: "operator toolchain guard rejected metadata".to_owned(),
            });
    }
    let incomplete = |problem: String| OrchestratorError::PreparationIncomplete {
        manifest: manifest.to_owned(),
        problem,
    };
    crate::inventory::require_rust_pin(catalog).map_err(|err| incomplete(err.to_string()))?;
    let request = MetadataDiscovery::new(prep.root.join(manifest))
        .map_err(|err| incomplete(err.to_string()))?;
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

fn check_creation_selection(release: &RustReleaseConfig, selection: &ReleaseSelection) -> Result<(), OrchestratorError> {
    if let Some(creation) = &release.new_crates {
        for (name, version) in &creation.packages {
            if !selection.packages.iter().any(|package| &package.name == name && &package.version == version) {
                return Err(OrchestratorError::config(CONFIG_REL, "stacks.rust.release.new_crates.packages", "creation_package_version_mismatch"));
            }
        }
    }
    Ok(())
}

fn check_recovery_record(
    release: &RustReleaseConfig,
    selection: &ReleaseSelection,
) -> Result<(), OrchestratorError> {
    let Some(record) = release.recovery.as_ref() else {
        return Ok(());
    };
    if selection.packages.len() != 1 {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.recovery.package",
            "recovery_requires_single_selected_package",
        ));
    }
    let selected = selection
        .packages
        .iter()
        .find(|package| package.name == record.package);
    let Some(selected) = selected else {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.recovery.package",
            "recovery_package_not_selected",
        ));
    };
    if selected.version != record.version {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "stacks.rust.release.recovery.version",
            format!("recovery_version_mismatch:{}", selected.version),
        ));
    }
    Ok(())
}

fn approved_source(
    root: &std::path::Path,
    release: &RustReleaseConfig,
) -> Result<String, OrchestratorError> {
    if let Some(record) = release.recovery.as_ref() {
        validate_source_sha(&record.source_sha)?;
        return Ok(record.source_sha.clone());
    }
    if let Some(creation) = &release.new_crates {
        validate_source_sha(&creation.source_sha)?;
        return Ok(creation.source_sha.clone());
    }
    head_sha(root)
}

fn release_plan(
    selection: &ReleaseSelection,
    repository: &str,
    sha: &str,
    plan_id: &str,
    registry: &str,
    version: Option<&str>,
    dispatch: bool,
) -> ReleasePlan {
    let packages: BTreeMap<String, String> = selection
        .packages
        .iter()
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect();
    ReleasePlan {
        plan_id: plan_id.to_owned(),
        repository: repository.to_owned(),
        source_sha: sha.to_owned(),
        registry: registry.to_owned(),
        packages,
        version: version.map(str::to_owned),
        dispatch,
    }
}

fn release_triggers(branch: &str, plan: &ReleasePlan) -> ReleaseTriggers {
    if !plan.dispatch {
        return ReleaseTriggers {
            push_branches: vec![branch.to_owned()],
            schedule: None,
            dispatch_inputs: Vec::new(),
        };
    }
    let mut dispatch_inputs = vec![
        DispatchInput {
            name: "plan".to_owned(),
            description: "Approved release plan".to_owned(),
            required: true,
            default: Some(plan.plan_id.clone()),
        },
        DispatchInput {
            name: "source_sha".to_owned(),
            description: "Approved source SHA".to_owned(),
            required: true,
            default: Some(plan.source_sha.clone()),
        },
    ];
    if let Some(version) = &plan.version {
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

fn reviewed_generator_manifest(raw: Option<&str>, stand_in: bool, version: &str) -> Result<(String, String), OrchestratorError> {
    use sha2::{Digest, Sha256};
    if stand_in {
        return Err(OrchestratorError::unsupported("generator_manifest", "release requires a reviewed consumer release manifest"));
    }
    let raw = raw.ok_or_else(|| OrchestratorError::unsupported("generator_manifest", "release requires a reviewed consumer release manifest"))?;
    let manifest = velnor_actions_workflow_renderer::release_helpers::validate_consumer_manifest_for_version(
        velnor_actions_contract::ReleaseTarget::LinuxX86_64, version, Some(raw),
    ).map_err(crate::local_release_manifest::map_release_helper_error)?;
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(raw.as_bytes()) {
        use std::fmt::Write as _;
        write!(&mut digest, "{byte:02x}").map_err(|_| OrchestratorError::Contract { problem: "generator_manifest_digest_encoding".to_owned() })?;
    }
    Ok((manifest.commit, digest))
}

#[cfg(test)]
#[path = "release_emit_tests.rs"]
mod tests;
