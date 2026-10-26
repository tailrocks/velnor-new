// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{
    ContractError, EdgeKind, ProposedTask, TaskEdge, TaskGraph, TaskNode, digest_b3,
};
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{local_edge_pairs, manifest_edges};

use crate::discover::Discovery;
use crate::git_paths::split_nul_paths;
use crate::schedule::resource_exclusions;
use crate::select_affected::manifest_dir;
use crate::validators::{validate_diff_rev, validate_select_diff_args, validate_select_show_args};

const MAX_BASE_MANIFEST_BATCH: usize = 512;

pub(crate) fn added_files(root: &Path, base: &str, head: &str) -> Result<BTreeSet<String>, String> {
    filtered_files(root, base, head, "A")
}

pub(crate) fn deleted_files(
    root: &Path,
    base: &str,
    head: &str,
) -> Result<BTreeSet<String>, String> {
    filtered_files(root, base, head, "D")
}

fn filtered_files(
    root: &Path,
    base: &str,
    head: &str,
    filter: &str,
) -> Result<BTreeSet<String>, String> {
    validate_diff_rev(base, "bad_base")?;
    validate_diff_rev(head, "bad_head")?;
    let range = format!("{base}...{head}");
    let mut args = vec![
        OsString::from("--name-only"),
        OsString::from("--no-renames"),
        OsString::from(format!("--diff-filter={filter}")),
        OsString::from(range),
        OsString::from("--"),
    ];
    validate_select_diff_args(&args).map_err(|err| err.to_string())?;
    args.insert(0, OsString::from("-z"));
    let output = GitRequest::diff(args)
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    split_nul_paths(&output.stdout)
}

pub(crate) fn head_edges(discovery: &Discovery) -> Vec<(String, String)> {
    let mut edges = Vec::new();
    for workspace in &discovery.workspaces {
        edges.extend(local_edge_pairs(&workspace.record.edges));
    }
    edges
}

pub(crate) fn base_edges(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
) -> Result<Vec<(String, String)>, String> {
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
    Ok(local_edge_pairs(&edges))
}

pub(crate) fn base_manifests(
    root: &Path,
    base: &str,
    manifests: &[&str],
) -> Result<Vec<String>, String> {
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

pub(crate) fn plan_task_graph(
    selected: &[&ProposedTask],
    lanes: &BTreeMap<String, u32>,
    digests: &BTreeMap<String, String>,
) -> Result<Vec<TaskEdge>, ContractError> {
    let ids: BTreeSet<&str> = selected.iter().map(|task| task.task_id.as_str()).collect();
    let mut nodes = Vec::with_capacity(selected.len());
    for task in selected {
        let mut node = task_node(task, lanes, digests);
        node.depends_on.retain(|dep| ids.contains(dep.as_str()));
        node.gated_by.retain(|gate| ids.contains(gate.as_str()));
        nodes.push(node);
    }
    nodes.sort_by(|left, right| left.task_id.cmp(&right.task_id));
    let mut edges = Vec::new();
    for task in selected {
        for dep in &task.depends_on {
            push_edge(&mut edges, &ids, dep, &task.task_id, EdgeKind::Data);
        }
        for gate in &task.gated_by {
            push_edge(&mut edges, &ids, gate, &task.task_id, EdgeKind::Gate);
        }
        if let Some((base, _, _)) = velnor_actions_contract::split_shard_suffix(&task.task_id) {
            push_edge(&mut edges, &ids, &task.task_id, base, EdgeKind::Report);
        }
    }
    exclusion_edges(selected, lanes, &mut edges);
    sort_dedupe_edges(&mut edges);
    let graph = TaskGraph { nodes, edges };
    graph.validate()?;
    Ok(graph.edges)
}

fn task_node(
    task: &ProposedTask,
    lanes: &BTreeMap<String, u32>,
    digests: &BTreeMap<String, String>,
) -> TaskNode {
    let lane = lanes.get(&task.task_id).copied().unwrap_or(0);
    let mut owned = task.clone();
    owned.depends_on.sort();
    owned.depends_on.dedup();
    owned.gated_by.sort();
    owned.gated_by.dedup();
    owned.into_task_node(
        digests.get(&task.task_id).cloned().unwrap_or_default(),
        digest_b3(lane.to_string().as_bytes()),
    )
}

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

fn exclusion_edges(
    selected: &[&ProposedTask],
    lanes: &BTreeMap<String, u32>,
    edges: &mut Vec<TaskEdge>,
) {
    let assignments: Vec<(&str, u32)> = selected
        .iter()
        .map(|task| {
            (
                task.task_id.as_str(),
                lanes.get(&task.task_id).copied().unwrap_or(0),
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

fn edge_rank(kind: EdgeKind) -> u8 {
    match kind {
        EdgeKind::Data => 0,
        EdgeKind::Gate => 1,
        EdgeKind::Report => 2,
        EdgeKind::ResourceExclusion => 3,
    }
}
