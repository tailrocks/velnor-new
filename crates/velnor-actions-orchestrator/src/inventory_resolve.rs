//! Locked/offline qualification plus the resolved graph it yields.

use std::path::Path;

use velnor_actions_mise::{MetadataQualification, ToolCatalog};
use velnor_actions_rust::WorkspaceRecord;

use super::require_rust_pin;
use crate::OrchestratorError;
use crate::discover::{PlannedWorkspace, workspace_lock, workspace_manifest};
use crate::generate::ToolSnapshot;

/// Full-resolve graph of one planned workspace, when one exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceResolve {
    /// Repository-relative workspace root; empty for the repository root.
    pub(crate) workspace_root: String,
    /// Parsed resolve, or the reason none exists (lockless, parse failure).
    pub(crate) graph: Result<velnor_actions_rust::ResolvedGraph, String>,
}

/// Complete graph of a lockless workspace, when its path edges are complete.
///
/// Without a lockfile there is no resolve to walk. A workspace whose
/// packages declare only path dependencies needs none: its path edges are
/// every edge. Any registry or git dependency could reach a patched local
/// crate, so such a workspace has no graph and selection fails closed.
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

/// Qualify locked/offline resolution where a lockfile pins deps.
///
/// Each qualified document is also parsed into the resolved package graph
/// affected-work selection walks. A lockless workspace or an unparsable
/// document records why it has no graph; selection then fails closed.
/// A tool snapshot brackets the runs, failing closed on tool drift.
/// # Errors
/// Returns `preparation_incomplete` when a lockfile cannot be qualified.
pub(crate) fn qualify_workspaces(
    root: &Path,
    workspaces: &[PlannedWorkspace],
    catalog: &ToolCatalog,
) -> Result<Vec<WorkspaceResolve>, OrchestratorError> {
    let tools = ToolSnapshot::capture(root);
    // Only rust workspaces qualify lockful metadata; an empty list never
    // spawns, so it must not require the pin.
    if !workspaces.is_empty() {
        require_rust_pin(catalog)?;
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
        let json = match request.run(catalog) {
            Ok(json) => json,
            Err(err) => {
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
