//! Base/head dependency-graph edges for affected-work selection.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{
    CachePolicy, ContractError, EdgeKind, ResourceClass, ResourceDemand, TaskEdge, TaskGraph,
    TaskNode, digest_b3,
};
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{DepKind, LocalEdge, STACK_ID, TaskGroup, TaskKind};

use crate::discover::Discovery;
use crate::internal_plan::{component_id_of, manifest_for_key};
use crate::schedule::resource_exclusions;
use crate::select_affected::manifest_dir;
use crate::validators::{validate_diff_rev, validate_select_diff_args, validate_select_show_args};

/// Max base manifests fetched; beyond this, broaden instead of reading.
const MAX_BASE_MANIFEST_BATCH: usize = 512;

/// Head paths added since base: manifests absent at base, skipped by batch.
///
/// Rename detection stays off so every head path missing at base reports as
/// added; the changed set keeps its own rename behavior untouched.
fn added_files(root: &Path, base: &str, head: &str) -> Result<BTreeSet<String>, String> {
    validate_diff_rev(base, "bad_base")?;
    validate_diff_rev(head, "bad_head")?;
    let range = format!("{base}...{head}");
    let args = vec![
        OsString::from("--name-only"),
        OsString::from("--no-renames"),
        OsString::from("--diff-filter=A"),
        OsString::from(range),
        OsString::from("--"),
    ];
    validate_select_diff_args(&args).map_err(|err| err.to_string())?;
    let output = GitRequest::diff(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let text = output.stdout_text("git").map_err(|err| err.to_string())?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Head local-path edges from discovery records.
pub(crate) fn head_edges(discovery: &Discovery) -> Vec<LocalEdge> {
    let mut edges = Vec::new();
    for workspace in &discovery.workspaces {
        edges.extend(workspace.record.edges.iter().cloned());
    }
    edges
}

/// Base local-path edges from base-revision manifests, in head-id space.
///
/// Every wanted manifest is read at `base` with one `git show` each; path
/// dependencies resolve to the head package owning the target directory, so
/// removed or renamed edges still select their head consumers. Manifests
/// added since base are new and contribute nothing; over-cap sets and
/// fetch or parse failures are errors that broaden via the caller.
pub(crate) fn base_edges(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
) -> Result<Vec<LocalEdge>, String> {
    let mut packages: Vec<(String, String)> = Vec::new();
    let mut wanted: Vec<(String, String)> = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external {
                packages.push((manifest_dir(&package.manifest), package.id.clone()));
                wanted.push((package.manifest.clone(), package.id.clone()));
            }
        }
    }
    let added = added_files(root, base, head)?;
    wanted.retain(|(manifest, _)| !added.contains(manifest));
    if wanted.len() > MAX_BASE_MANIFEST_BATCH {
        return Err("base_batch_over_cap".to_owned());
    }
    let specs: Vec<&str> = wanted
        .iter()
        .map(|(manifest, _)| manifest.as_str())
        .collect();
    let texts = base_manifests(root, base, &specs)?;
    let mut edges = Vec::new();
    for ((manifest, id), text) in wanted.iter().zip(texts.iter()) {
        edges.extend(manifest_edges(
            text,
            id,
            &manifest_dir(manifest),
            &packages,
        )?);
    }
    Ok(edges)
}

/// Base contents of every wanted manifest via one `git show` each.
///
/// One batched `git show` cannot delimit blobs: git shows a repeated
/// separator object only once, so per-manifest reads keep boundaries exact.
/// Added (new) manifests never reach here.
fn base_manifests(root: &Path, base: &str, manifests: &[&str]) -> Result<Vec<String>, String> {
    validate_diff_rev(base, "bad_base")?;
    let mut out = Vec::with_capacity(manifests.len());
    for manifest in manifests {
        let args = vec![OsString::from(format!("{base}:{manifest}"))];
        validate_select_show_args(&args).map_err(|err| err.to_string())?;
        let output = GitRequest::show(args)
            .run_in(root)
            .map_err(|err| err.to_string())?;
        output
            .require_success("git")
            .map_err(|err| err.to_string())?;
        out.push(output.stdout_text("git").map_err(|err| err.to_string())?);
    }
    Ok(out)
}

/// Path edges of one base manifest, resolved to head package IDs.
fn manifest_edges(
    text: &str,
    from: &str,
    dir: &str,
    packages: &[(String, String)],
) -> Result<Vec<LocalEdge>, String> {
    let document: toml::Table = toml::from_str(text).map_err(|err| err.to_string())?;
    let mut edges = Vec::new();
    for (table, kind) in sections() {
        if let Some(deps) = document.get(table).and_then(toml::Value::as_table) {
            edges.extend(dep_edges(deps, from, dir, packages, kind, None));
        }
    }
    if let Some(targets) = document.get("target").and_then(toml::Value::as_table) {
        for (name, target) in targets {
            let Some(target) = target.as_table() else {
                continue;
            };
            for (table, kind) in sections() {
                if let Some(deps) = target.get(table).and_then(toml::Value::as_table) {
                    edges.extend(dep_edges(deps, from, dir, packages, kind, Some(name)));
                }
            }
        }
    }
    Ok(edges)
}

/// Dependency tables with their edge kinds.
fn sections() -> [(&'static str, DepKind); 3] {
    [
        ("dependencies", DepKind::Normal),
        ("build-dependencies", DepKind::Build),
        ("dev-dependencies", DepKind::Dev),
    ]
}

/// Path-dep edges of one dependency table.
fn dep_edges(
    deps: &toml::Table,
    from: &str,
    dir: &str,
    packages: &[(String, String)],
    kind: DepKind,
    target: Option<&str>,
) -> Vec<LocalEdge> {
    let mut edges = Vec::new();
    for spec in deps.values() {
        let Some(spec) = spec.as_table() else {
            continue;
        };
        let Some(path) = spec.get("path").and_then(toml::Value::as_str) else {
            continue;
        };
        let joined = join_dir(dir, path);
        let Some(to) = packages
            .iter()
            .find(|(owned, _)| *owned == joined)
            .map(|(_, id)| id)
        else {
            continue;
        };
        let optional = spec
            .get("optional")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        edges.push(LocalEdge {
            from: from.to_owned(),
            to: to.clone(),
            kind,
            optional,
            target: target.map(str::to_owned),
        });
    }
    edges
}

/// Join a manifest directory with a dep path, resolving `.` and `..`.
fn join_dir(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|seg| !seg.is_empty()).collect();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    parts.join("/")
}

/// Validated plan graph: nodes per group plus sorted unique edges (PAR-3.1/3.2).
///
/// Data edges follow `depends_on`, gate edges follow `gated_by`, report
/// edges bind shard legs to their base task, and resource exclusions come
/// from the lane assignment. Endpoints outside the selection are dropped:
/// plan edges must name planned obligations.
pub(crate) fn plan_task_graph(
    selected: &[&TaskGroup],
    lanes: &BTreeMap<String, u32>,
    digests: &BTreeMap<String, String>,
) -> Result<Vec<TaskEdge>, ContractError> {
    let ids: BTreeSet<&str> = selected
        .iter()
        .map(|group| group.task_id.as_str())
        .collect();
    let mut nodes = Vec::with_capacity(selected.len());
    for group in selected {
        let mut node = task_node(group, lanes, digests);
        node.depends_on.retain(|dep| ids.contains(dep.as_str()));
        node.gated_by.retain(|gate| ids.contains(gate.as_str()));
        nodes.push(node);
    }
    nodes.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    let mut edges = Vec::new();
    for group in selected {
        for dep in &group.depends_on {
            push_edge(&mut edges, &ids, dep, &group.task_id, EdgeKind::Data);
        }
        for gate in &group.gated_by {
            push_edge(&mut edges, &ids, gate, &group.task_id, EdgeKind::Gate);
        }
        if let Some((base, _)) = group.task_id.split_once("/shard-") {
            push_edge(&mut edges, &ids, &group.task_id, base, EdgeKind::Report);
        }
    }
    exclusion_edges(selected, lanes, &mut edges);
    sort_dedupe_edges(&mut edges);
    let graph = TaskGraph { nodes, edges };
    graph.validate()?;
    Ok(graph.edges)
}

/// One graph node for one selected group.
fn task_node(
    group: &TaskGroup,
    lanes: &BTreeMap<String, u32>,
    digests: &BTreeMap<String, String>,
) -> TaskNode {
    let lane = lanes.get(&group.task_id).copied().unwrap_or(0);
    let mut depends_on = group.depends_on.clone();
    depends_on.sort();
    depends_on.dedup();
    let mut gated_by = group.gated_by.clone();
    gated_by.sort();
    gated_by.dedup();
    TaskNode {
        task_id: group.task_id.clone(),
        stack_id: STACK_ID.to_owned(),
        component_id: component_id_of(&group.package_id),
        task_kind: group.kind.as_str().to_owned(),
        configuration: group.configuration.clone(),
        input_digest: digests.get(&group.task_id).cloned().unwrap_or_default(),
        depends_on,
        gated_by,
        reads: vec![manifest_for_key(&group.manifest_key)],
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: resource_class_for(group.kind),
            cpu_milli: None,
            memory_mb: None,
            needs_network: group.uses_network,
            service: None,
        },
        lane_id: digest_b3(lane.to_string().as_bytes()),
        cache_policy: CachePolicy {
            allow_compilation_reuse: true,
            allow_task_reuse: !group.undeclared_reads,
        },
    }
}

/// Resource class for one task kind.
fn resource_class_for(kind: TaskKind) -> ResourceClass {
    match kind {
        TaskKind::Clippy | TaskKind::Build => ResourceClass::Compiler,
        TaskKind::Test | TaskKind::Nextest | TaskKind::Doctest => ResourceClass::Test,
        TaskKind::Doc | TaskKind::Fmt => ResourceClass::Lightweight,
    }
}

/// Push one edge when both endpoints are planned and distinct.
fn push_edge(
    edges: &mut Vec<TaskEdge>,
    ids: &BTreeSet<&str>,
    from: &str,
    to: &str,
    kind: EdgeKind,
) {
    if from != to && ids.contains(from) && ids.contains(to) {
        edges.push(TaskEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            kind,
        });
    }
}

/// Resource-exclusion edges for lane-sharing pairs (PAR-7.2).
fn exclusion_edges(
    selected: &[&TaskGroup],
    lanes: &BTreeMap<String, u32>,
    edges: &mut Vec<TaskEdge>,
) {
    let assignments: Vec<(&str, u32)> = selected
        .iter()
        .map(|group| {
            (
                group.task_id.as_str(),
                lanes.get(&group.task_id).copied().unwrap_or(0),
            )
        })
        .collect();
    for (left, right) in resource_exclusions(&assignments) {
        edges.push(TaskEdge {
            from: left,
            to: right,
            kind: EdgeKind::ResourceExclusion,
        });
    }
}

/// Sort edges by (`from`, `to`, kind) and drop duplicates.
fn sort_dedupe_edges(edges: &mut Vec<TaskEdge>) {
    edges.sort_by(|left, right| {
        (left.from.as_str(), left.to.as_str(), edge_rank(left.kind)).cmp(&(
            right.from.as_str(),
            right.to.as_str(),
            edge_rank(right.kind),
        ))
    });
    edges.dedup_by(|curr, prev| {
        curr.from == prev.from && curr.to == prev.to && curr.kind == prev.kind
    });
}

/// Kind rank mirroring the contract's deterministic edge order.
fn edge_rank(kind: EdgeKind) -> u8 {
    match kind {
        EdgeKind::Data => 0,
        EdgeKind::Gate => 1,
        EdgeKind::Report => 2,
        EdgeKind::ResourceExclusion => 3,
    }
}
