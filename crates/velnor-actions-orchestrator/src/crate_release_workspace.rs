// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, ffi::OsString, path::Path, time::Duration};
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_mise::{GitRequest, MetadataDiscovery, PinnedTool, PinnedToolExec};
use velnor_actions_rust::{
    release_graph::{RegistryState, ReleasePlan, plan_release, registry_requirements},
    release_select::{ReleaseRequest, ReleaseScope, select_release_set},
};

use crate::{
    OrchestratorError,
    crate_release_source::{
        source_cargo_path, source_compiler_env, source_tool_catalog, verify_crate_release_source,
        verify_toolchain_source_separation,
    },
    internal::internal,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateReleaseWorkspacePlan {
    pub plan: ReleasePlan,
    pub registry_evidence: Vec<crate::crate_registry_inventory::CrateRegistryInventory>,
    pub(crate) concurrency_group: String,
}

pub fn crate_release_workspace_plan_with_registry<
    R: crate::crate_release_registry::CrateRegistryRead,
>(
    root: &Path,
    sha: &str,
    intent: &CrateReleaseIntent,
    registry: &R,
) -> Result<CrateReleaseWorkspacePlan, OrchestratorError> {
    intent
        .validate()
        .map_err(|_| internal("crate_release_intent_invalid"))?;
    verify_crate_release_source(root, sha)?;
    verify_toolchain_source_separation(root)?;
    let canonical = root
        .canonicalize()
        .map_err(|_| internal("crate_release_source_root_invalid"))?;
    for ancestor in canonical.ancestors().skip(1) {
        for name in ["config", "config.toml"] {
            match std::fs::symlink_metadata(ancestor.join(".cargo").join(name)) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(internal("crate_release_ambient_cargo_config")),
            }
        }
    }
    let manifests = tracked_manifests(&canonical)?;
    if !manifests.contains(&intent.manifest_path) {
        return Err(internal("crate_release_manifest_untracked"));
    }
    let metadata = workspace_metadata(&canonical, intent)?;
    verify_crate_release_source(&canonical, sha)?;
    let concurrency_group = crate::release_identity::workspace_lock(
        &metadata,
        &canonical,
        &intent.manifest_path,
        &manifests,
        &intent.registry,
        &intent.repository,
    )?
    .group;
    let scope = ReleaseScope::Packages(intent.packages.clone());
    let affected = BTreeSet::new();
    let request = ReleaseRequest {
        metadata_json: &metadata,
        repo_root: &canonical,
        manifest_hint: &intent.manifest_path,
        scope: &scope,
        enabled: true,
        affected: &affected,
        known_manifests: &manifests,
        supported_registries: &[],
    };
    let (state, registry_evidence) = registry_state(&request, registry)?;
    let plan = plan_release(&request, &state)
        .map_err(|_| internal("crate_release_workspace_plan_invalid"))?;
    if plan
        .selection
        .packages
        .iter()
        .any(|package| !manifests.contains(&package.manifest))
    {
        return Err(internal("crate_release_manifest_untracked"));
    }
    if let Some(recovery) = &intent.recovery
        && (plan.selection.packages.len() != 1
            || plan.selection.packages[0].name != recovery.package
            || plan.selection.packages[0].version != recovery.version
            || sha != recovery.source_sha)
    {
        return Err(internal("crate_release_recovery_mismatch"));
    }
    Ok(CrateReleaseWorkspacePlan {
        plan,
        registry_evidence,
        concurrency_group,
    })
}

fn tracked_manifests(root: &Path) -> Result<BTreeSet<String>, OrchestratorError> {
    let tracked = GitRequest::ls_files(vec!["-z".into()])
        .run_release_in(root)
        .map_err(|_| internal("crate_release_source_git"))?;
    if !tracked.success {
        return Err(internal("crate_release_source_git"));
    }
    let mut manifests = BTreeSet::new();
    for entry in tracked
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let path = std::str::from_utf8(entry)
            .map_err(|_| internal("crate_release_source_path_unsupported"))?;
        if Path::new(path)
            .file_name()
            .is_some_and(|name| name == "Cargo.toml")
        {
            manifests.insert(path.to_owned());
        }
    }
    Ok(manifests)
}

fn registry_state<R: crate::crate_release_registry::CrateRegistryRead>(
    request: &ReleaseRequest<'_>,
    registry: &R,
) -> Result<
    (
        RegistryState,
        Vec<crate::crate_registry_inventory::CrateRegistryInventory>,
    ),
    OrchestratorError,
> {
    let selection = select_release_set(request)
        .map_err(|_| internal("crate_release_workspace_plan_invalid"))?;
    let required = registry_requirements(&selection)
        .map_err(|_| internal("crate_release_workspace_plan_invalid"))?;
    let mut state = RegistryState::default();
    let mut evidence = Vec::new();
    for key in required {
        match crate::crate_registry_inventory::inspect_crate_versions(registry, &key.package) {
            Ok(inventory) => {
                state.published.insert(key, inventory.available_versions());
                evidence.push(inventory);
            }
            Err(crate::crate_registry_inventory::CrateInventoryError::NotFound) => {}
            Err(_) => return Err(internal("crate_release_registry_evidence_invalid")),
        }
    }
    Ok((state, evidence))
}

fn workspace_metadata(
    root: &Path,
    intent: &CrateReleaseIntent,
) -> Result<String, OrchestratorError> {
    let temporary = tempfile::tempdir().map_err(|_| internal("crate_release_metadata_temp"))?;
    let cargo_home = temporary.path().join("cargo-home");
    std::fs::create_dir(&cargo_home).map_err(|_| internal("crate_release_metadata_temp"))?;
    let discovery = MetadataDiscovery::new(root.join(&intent.manifest_path))
        .map_err(|_| internal("crate_release_metadata_failed"))?;
    let mut args = vec![
        OsString::from("-i"),
        format!("HOME={}", temporary.path().display()).into(),
        format!("CARGO_HOME={}", cargo_home.display()).into(),
        "CARGO_NET_OFFLINE=true".into(),
    ];
    args.extend(source_compiler_env(&intent.rust_version)?);
    args.push(source_cargo_path(&intent.rust_version, temporary.path())?.into_os_string());
    args.extend(discovery.cargo_argv().into_iter().skip(1));
    let exec = PinnedToolExec::new(vec![PinnedTool::Rust], std::ffi::OsStr::new("env"), args)
        .map_err(|_| internal("crate_release_metadata_failed"))?;
    let command = exec
        .command(
            &source_tool_catalog(&intent.rust_version)
                .map_err(|_| internal("crate_release_toolchain_invalid"))?,
        )
        .map_err(|_| internal("crate_release_metadata_failed"))?
        .with_cwd(root.to_path_buf());
    let output = command
        .run_bounded(16 * 1024 * 1024, Duration::from_secs(60))
        .map_err(|_| internal("crate_release_metadata_failed"))?;
    if !output.success {
        return Err(internal("crate_release_metadata_failed"));
    }
    output
        .stdout_text("cargo")
        .map_err(|_| internal("crate_release_metadata_failed"))
}
