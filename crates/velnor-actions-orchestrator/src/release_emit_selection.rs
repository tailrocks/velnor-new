// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, prepare::GenerationPreparation};
use std::collections::BTreeSet;
use velnor_actions_contract::config::{ReleaseRegistry, RustReleaseConfig};
use velnor_actions_mise::{GitRequest, MetadataDiscovery};
use velnor_actions_rust::release_select::{
    ReleaseRequest as SelectRequest, ReleaseScope, ReleaseSelection, select_release_set,
};

pub(crate) fn select_from_json(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
    json: &str,
) -> Result<ReleaseSelection, OrchestratorError> {
    let known_manifests = tracked_manifests(&prep.root)?;
    select_from_json_with_known(prep, release, json, &known_manifests)
}

pub(super) fn select_from_json_with_known(
    prep: &GenerationPreparation,
    release: &RustReleaseConfig,
    json: &str,
    known_manifests: &BTreeSet<String>,
) -> Result<ReleaseSelection, OrchestratorError> {
    select_with_root(&prep.root, release, json, known_manifests)
}

pub(super) fn select_with_root(
    root: &std::path::Path,
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
        repo_root: root,
        manifest_hint: manifest,
        scope: &scope,
        enabled: true,
        affected: &affected,
        known_manifests,
        supported_registries: supported,
    };
    let record =
        velnor_actions_rust::metadata::parse_metadata_json(json, root, manifest, known_manifests)
            .map_err(|err| OrchestratorError::Contract {
            problem: err.to_string(),
        })?;
    let workspace_manifest = if record.workspace_root.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{}/Cargo.toml", record.workspace_root)
    };
    if !velnor_actions_rust::tracked_manifest_in_source(root, known_manifests, manifest)
        || !velnor_actions_rust::tracked_manifest_in_source(
            root,
            known_manifests,
            &workspace_manifest,
        )
        || record.packages.iter().any(|package| {
            !package.external
                && !velnor_actions_rust::tracked_manifest_in_source(
                    root,
                    known_manifests,
                    &package.manifest,
                )
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

pub(super) fn tracked_manifests(
    root: &std::path::Path,
) -> Result<BTreeSet<String>, OrchestratorError> {
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
    release_metadata_json_at(prep, &prep.root, &prep.catalog, manifest)
}

pub(super) fn release_metadata_json_at(
    prep: &GenerationPreparation,
    root: &std::path::Path,
    catalog: &velnor_actions_mise::ToolCatalog,
    manifest: &str,
) -> Result<String, OrchestratorError> {
    if let Some(guard) = prep.operator_toolchain_guard() {
        crate::inventory::require_metadata_tools(catalog, Some(guard))?;
        return MetadataDiscovery::new(root.join(manifest))
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
