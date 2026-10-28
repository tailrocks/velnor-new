// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use velnor_actions_mise::{MetadataQualification, OperatorToolchainGuard, ToolCatalog};
use velnor_actions_rust::WorkspaceRecord;

use super::require_metadata_tools;
use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};
use crate::generate::ToolSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceResolve {
    pub(crate) workspace_root: String,
    pub(crate) graph: Result<velnor_actions_rust::ResolvedGraph, String>,
}

fn lockless_graph(record: &WorkspaceRecord) -> Result<velnor_actions_rust::ResolvedGraph, String> {
    if record.has_external_dependencies {
        return Err("lockless_workspace".to_owned());
    }
    let mut path_packages: Vec<velnor_actions_rust::ResolvedPathPackage> = record
        .packages
        .iter()
        .filter(|package| !package.external)
        .map(|package| velnor_actions_rust::ResolvedPathPackage {
            id: package.id.clone(),
            dir: crate::select_affected::manifest_dir(&package.manifest),
        })
        .collect();
    path_packages.sort();
    let mut edges = velnor_actions_rust::local_edge_pairs(&record.edges);
    edges.sort();
    edges.dedup();
    Ok(velnor_actions_rust::ResolvedGraph {
        edges,
        path_packages,
    })
}

pub(crate) fn qualify_workspaces_with_guard(
    root: &Path,
    workspaces: &[PlannedWorkspace],
    catalog: &ToolCatalog,
    guard: Option<&OperatorToolchainGuard>,
) -> Result<Vec<WorkspaceResolve>, OrchestratorError> {
    let tools = ToolSnapshot::capture(root);
    if !workspaces.is_empty() {
        require_metadata_tools(catalog, guard)?;
    }
    let mut resolves = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        let prefix = workspace.record.workspace_root.clone();
        let lock = workspace_lock(&prefix);
        if !root.join(&lock).is_file() {
            resolves.push(WorkspaceResolve {
                workspace_root: prefix,
                graph: lockless_graph(&workspace.record),
            });
            continue;
        }
        let manifest = workspace_manifest(&prefix);
        let request = MetadataQualification::new(root.join(&manifest)).map_err(|err| {
            OrchestratorError::PreparationIncomplete {
                manifest: manifest.clone(),
                problem: format!("bad_manifest_path:{err}"),
            }
        })?;
        let result = match guard {
            Some(guard) => request.run_with_operator_guard(catalog, guard),
            None => request.run(catalog),
        };
        let json = match result {
            Ok(json) => json,
            Err(err) => {
                if guard.is_some() {
                    return Err(OrchestratorError::ToolchainGuard {
                        problem: "operator toolchain guard rejected metadata".to_owned(),
                    });
                }
                let text = err.to_string();
                let first = text.lines().next().unwrap_or("metadata_offline");
                let short: String = first.chars().take(160).collect();
                return Err(OrchestratorError::PreparationIncomplete {
                    manifest,
                    problem: format!("metadata_offline:{short}"),
                });
            }
        };
        let graph = velnor_actions_rust::parse_resolved_graph(&json, root, &manifest)
            .map_err(|err| format!("unparsable_resolve:{err}"));
        resolves.push(WorkspaceResolve {
            workspace_root: prefix,
            graph,
        });
    }
    tools.verify(root)?;
    Ok(resolves)
}
