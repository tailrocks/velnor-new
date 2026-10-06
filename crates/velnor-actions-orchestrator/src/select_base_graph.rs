//! Immutable base Cargo inventories and manifest-bound graph translation.

#[path = "select_base_tree.rs"]
mod tree;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::build_index_from_list;
use velnor_actions_mise::{MetadataDiscovery, ToolCatalog};
use velnor_actions_rust::{WorkspaceRecord, parse_metadata_json};

use crate::discover::Discovery;
use crate::select_affected::manifest_dir;

/// Dependency graph with the complete package ownership inventory.
#[derive(Default)]
pub(crate) struct PackageGraph {
    pub(crate) owners: Vec<(String, String)>,
    pub(crate) edges: Vec<(String, String)>,
}

/// Candidate graph, including declared cross-workspace path consumers.
pub(crate) fn candidate_graph(root: &Path, discovery: &Discovery) -> Result<PackageGraph, String> {
    let records: Vec<_> = discovery
        .workspaces
        .iter()
        .map(|workspace| &workspace.record)
        .collect();
    let mut manifests = BTreeSet::new();
    for record in &records {
        manifests.insert(crate::discover::workspace_manifest(&record.workspace_root));
        manifests.extend(
            record
                .packages
                .iter()
                .map(|package| package.manifest.clone()),
        );
    }
    qualify_resolution(root, &manifests)?;
    graph_from_records(root, &records)
}

/// Cargo interprets base membership, inherited dependencies, and all edge kinds.
///
/// A provider-authenticated exact-base inventory with unchanged resolution
/// inputs supplies those same complete records without another Cargo request.
/// Its paths are rebound to `root`; candidate manifest qualification still runs.
/// No source executes: immutable manifests and target path presence feed the
/// existing pinned `metadata --no-deps` request. Configured discovery exclusions
/// apply to both inventories; a changed Velnor config broadens before this call.
pub(crate) fn base_graph(
    root: &Path,
    base: &str,
    discovery: &Discovery,
) -> Result<PackageGraph, String> {
    if let Some((inventory, records)) = discovery
        .rust_inventory
        .as_ref()
        .filter(|inventory| inventory.applies_to(root))
        .and_then(|inventory| {
            inventory
                .base_records(base)
                .map(|records| (inventory, records))
        })
    {
        let config = crate::config::load_config(root).map_err(|error| error.to_string())?;
        let (index, skipped) =
            crate::discover_index::build_file_index(root, &config.discovery.exclude)
                .map_err(|error| error.to_string())?;
        if skipped {
            return Err("analysis_inventory_incomplete_path_index".to_owned());
        }
        inventory.validate_current(root, index.files())?;
        let candidate = candidate_graph(root, discovery)?;
        let references: Vec<_> = records.iter().collect();
        let graph = graph_from_records(root, &references)?;
        return remap_graph(root, root, graph, candidate);
    }
    if discovery.workspaces.is_empty() {
        return Ok(PackageGraph::default());
    }
    let (tree, manifests) = tree::materialize(root, base)?;
    let temp_root = tree
        .path()
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let config = crate::config::load_config(root).map_err(|error| error.to_string())?;
    let index = build_index_from_list(&temp_root, &manifests, &config.discovery.exclude)
        .map_err(|error| error.to_string())?;
    let manifests: BTreeSet<_> = velnor_actions_rust::discover_stack_candidates(&index)
        .into_iter()
        .map(|unit| velnor_actions_rust::manifest_for_unit_root(&unit.unit_root))
        .collect();
    let records = inventories(&temp_root, &manifests)?;
    let references: Vec<_> = records.iter().collect();
    let graph = graph_from_records(&temp_root, &references)?;
    let candidate = candidate_graph(root, discovery)?;
    remap_graph(root, &temp_root, graph, candidate)
}

/// Surviving manifests keep candidate Cargo IDs; removed packages remain owners.
fn remap_graph(
    root: &Path,
    temp_root: &Path,
    mut graph: PackageGraph,
    candidate: PackageGraph,
) -> Result<PackageGraph, String> {
    let by_dir: BTreeMap<_, _> = candidate.owners.into_iter().collect();
    let translations: BTreeMap<_, _> = graph
        .owners
        .iter()
        .map(|(dir, id)| {
            let selected = by_dir.get(dir).cloned().unwrap_or_else(|| {
                format!(
                    "base:{}",
                    id.replace(
                        &temp_root.to_string_lossy().to_string(),
                        &root.to_string_lossy()
                    )
                )
            });
            (id.clone(), selected)
        })
        .collect();
    for (_, id) in &mut graph.owners {
        *id = translated(&translations, id)?;
    }
    for (from, to) in &mut graph.edges {
        *from = translated(&translations, from)?;
        *to = translated(&translations, to)?;
    }
    Ok(graph)
}

/// Inventory each immutable workspace once; Cargo confirms every reused member.
fn inventories(root: &Path, manifests: &BTreeSet<String>) -> Result<Vec<WorkspaceRecord>, String> {
    qualify_resolution(root, manifests)?;
    let catalog = ToolCatalog::pinned();
    let cargo_home = tempfile::tempdir().map_err(|error| error.to_string())?;
    let cargo_env = [
        (
            OsString::from("CARGO_HOME"),
            cargo_home.path().as_os_str().to_owned(),
        ),
        (
            OsString::from("MISE_CARGO_HOME"),
            cargo_home.path().as_os_str().to_owned(),
        ),
        (OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")),
    ];
    let mut records = Vec::new();
    let mut members = BTreeSet::new();
    for manifest in manifests {
        if members.contains(manifest) {
            continue;
        }
        let request =
            MetadataDiscovery::new(root.join(manifest)).map_err(|error| error.to_string())?;
        let output = request
            .command(&catalog)
            .map_err(|error| error.to_string())?
            .with_cwd(root.to_path_buf())
            .with_env(&cargo_env)
            .map_err(|error| error.to_string())?
            .run()
            .map_err(|error| error.to_string())?;
        if !output.success {
            return Err(format!("base_metadata_unavailable:{manifest}"));
        }
        let json = output
            .stdout_text("mise")
            .map_err(|error| error.to_string())?;
        let record = parse_metadata_json(&json, root, manifest, manifests)
            .map_err(|error| error.to_string())?;
        members.extend(
            record
                .packages
                .iter()
                .filter(|package| package.in_workspace)
                .map(|package| package.manifest.clone()),
        );
        records.push(record);
    }
    Ok(velnor_actions_rust::dedupe_workspaces(records))
}

/// Incomplete resolution inputs always broaden; no override edge is omitted.
fn qualify_resolution(root: &Path, manifests: &BTreeSet<String>) -> Result<(), String> {
    let mut directories = BTreeSet::from([String::new()]);
    for manifest in manifests {
        let text = match crate::safe_read::read_repo_file(
            root,
            manifest,
            crate::safe_read::MAX_REPO_FILE_BYTES,
        )
        .map_err(|error| error.to_string())?
        {
            crate::safe_read::RepoRead::Text(text) => text,
            crate::safe_read::RepoRead::Absent => {
                return Err(format!("graph_manifest_absent:{manifest}"));
            }
        };
        velnor_actions_rust::qualify_manifest_graph(&text)?;
        let mut parent = Path::new(manifest).parent();
        while let Some(dir) = parent {
            directories.insert(dir.to_string_lossy().to_string());
            parent = dir.parent();
        }
    }
    for dir in directories {
        reject_cargo_config(&root.join(dir))?;
    }
    let canonical = root.canonicalize().map_err(|error| error.to_string())?;
    for ancestor in canonical.ancestors() {
        reject_cargo_config(ancestor)?;
    }
    Ok(())
}

/// Cargo also searches ancestor directories; symlinks are unresolved inputs.
fn reject_cargo_config(dir: &Path) -> Result<(), String> {
    for config in ["config", "config.toml"] {
        match std::fs::symlink_metadata(dir.join(".cargo").join(config)) {
            Ok(_) => return Err("cargo_config_requires_resolution".to_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("cargo_config_unreadable".to_owned()),
        }
    }
    Ok(())
}

/// Resolve every path edge across all workspace records, never silently skip.
fn graph_from_records(root: &Path, records: &[&WorkspaceRecord]) -> Result<PackageGraph, String> {
    let mut graph = PackageGraph::default();
    for record in records {
        for package in &record.packages {
            if package.external {
                return Err("unresolved_path_edge:outside_root".to_owned());
            }
            if package.in_workspace && !package.external {
                graph
                    .owners
                    .push((manifest_dir(&package.manifest), package.id.clone()));
            }
        }
    }
    graph.owners.sort();
    graph.owners.dedup();
    let by_dir: BTreeMap<_, _> = graph.owners.iter().cloned().collect();
    for record in records {
        graph
            .edges
            .extend(velnor_actions_rust::local_edge_pairs(&record.edges));
        for edge in &record.skipped_edges {
            let relative = Path::new(&edge.path)
                .strip_prefix(root)
                .map_err(|_| "unresolved_path_edge:outside_root".to_owned())?;
            let dir = relative
                .to_str()
                .ok_or_else(|| "unresolved_path_edge:non_utf8".to_owned())?;
            let to = by_dir
                .get(dir)
                .ok_or_else(|| format!("unresolved_path_edge:{dir}"))?;
            graph.edges.push((edge.from.clone(), to.clone()));
        }
    }
    graph.edges.sort();
    graph.edges.dedup();
    let ids: BTreeSet<_> = graph.owners.iter().map(|(_, id)| id).collect();
    if graph
        .edges
        .iter()
        .any(|(from, to)| !ids.contains(from) || !ids.contains(to))
    {
        return Err("unresolved_path_edge:missing_package".to_owned());
    }
    Ok(graph)
}

/// Every edge endpoint must belong to the inventoried manifest identity map.
fn translated(map: &BTreeMap<String, String>, id: &str) -> Result<String, String> {
    map.get(id)
        .cloned()
        .ok_or_else(|| "base_graph_missing_package_identity".to_owned())
}

#[cfg(test)]
#[path = "select_base_graph_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "select_base_inventory_tests.rs"]
mod inventory_tests;
