// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{DetectionStatus, ProposedTask};
use velnor_actions_tofu::{
    Family, ModuleEdges, chdir_finding_for_root, family_of, key_for_root, select_roots,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::select_edges::{added_files, base_manifests, deleted_files};

#[derive(Debug, Clone)]
pub struct TofuSelectionUnit {
    pub root: String,
    pub files: Vec<String>,
    pub edges: ModuleEdges,
}

const MAX_TOFU_BASE_FILE_BATCH: usize = 512;

pub(crate) fn tofu_selected_roots(statuses: &[DetectionStatus]) -> Vec<String> {
    let mut roots: Vec<String> = statuses
        .iter()
        .filter_map(|status| match status {
            DetectionStatus::Selected(project)
                if project.stack_id == velnor_actions_tofu::STACK_ID =>
            {
                Some(project.project_root.clone())
            }
            _ => None,
        })
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

pub(crate) fn push_chdir_findings(
    discovery: &Discovery,
    warnings: &mut Vec<String>,
) -> Result<(), OrchestratorError> {
    let mut findings: BTreeSet<String> = BTreeSet::new();
    for task in &discovery.proposals {
        if task.stack_id != velnor_actions_tofu::STACK_ID {
            continue;
        }
        let root = velnor_actions_tofu::normalized_root_for_proposal(task)?;
        if let Some(finding) = chdir_finding_for_root(root) {
            findings.insert(finding);
        }
    }
    warnings.extend(findings);
    Ok(())
}

pub(crate) fn select_tofu_keys(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Result<BTreeSet<String>, String> {
    let head_roots = tofu_selected_roots(&discovery.statuses);
    if head_roots.is_empty() {
        return Ok(BTreeSet::new());
    }
    let head_graph = head_module_graph(discovery);
    let base_graph = base_module_graph(root, base, head, discovery)?;
    let nodes = union_nodes(&head_roots, &base_graph, &head_graph);
    let tofu_changed = tofu_owned(changed, &nodes);
    let selection = select_roots(
        &head_roots,
        &head_roots,
        &base_graph,
        &head_graph,
        &tofu_changed,
    )
    .map_err(|err| err.to_string())?;
    for reason in &selection.fallback {
        warnings.push(format!("tofu_select_all:{reason}"));
    }
    Ok(selection
        .selected
        .into_iter()
        .map(|root| key_for_root(&root))
        .collect())
}

pub(crate) fn tofu_keys_or_broaden(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Option<BTreeSet<String>> {
    match select_tofu_keys(root, base, head, discovery, changed, warnings) {
        Ok(keys) => Some(keys),
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            None
        }
    }
}

fn head_module_graph(discovery: &Discovery) -> ModuleEdges {
    let mut edges = Vec::new();
    let mut findings = Vec::new();
    for unit in &discovery.tofu_units {
        edges.extend(unit.edges.edges.iter().cloned());
        findings.extend(unit.edges.findings.iter().cloned());
    }
    edges.sort_by(|left, right| {
        (&left.from, &left.to, &left.source).cmp(&(&right.from, &right.to, &right.source))
    });
    edges.dedup_by(|curr, prev| {
        curr.from == prev.from && curr.to == prev.to && curr.source == prev.source
    });
    findings.sort_by(|left, right| {
        (&left.file, &left.name, &left.detail).cmp(&(&right.file, &right.name, &right.detail))
    });
    findings.dedup_by(|curr, prev| {
        curr.file == prev.file
            && curr.name == prev.name
            && curr.class == prev.class
            && curr.detail == prev.detail
    });
    ModuleEdges { edges, findings }
}

fn base_module_graph(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
) -> Result<ModuleEdges, String> {
    let mut wanted: BTreeSet<String> = discovery
        .tofu_units
        .iter()
        .flat_map(|unit| unit.files.iter().cloned())
        .collect();
    let added = added_files(root, base, head)?;
    wanted.retain(|path| !added.contains(path));
    let deleted = deleted_files(root, base, head)?;
    wanted.extend(deleted.into_iter().filter(|path| {
        let name = path.rsplit('/').next().unwrap_or(path);
        matches!(family_of(name), Family::Config | Family::Override)
    }));
    if wanted.len() > MAX_TOFU_BASE_FILE_BATCH {
        return Err("tofu_base_batch_over_cap".to_owned());
    }
    let specs: Vec<&str> = wanted.iter().map(String::as_str).collect();
    let texts = base_manifests(root, base, &specs)?;
    let pairs: Vec<(String, String)> = wanted.into_iter().zip(texts).collect();
    let refs = velnor_actions_tofu::module_refs_for_texts(&pairs).map_err(|err| err.to_string())?;
    velnor_actions_tofu::resolve_refs(&refs).map_err(|err| err.to_string())
}

fn union_nodes(roots: &[String], base: &ModuleEdges, head: &ModuleEdges) -> BTreeSet<String> {
    let mut nodes: BTreeSet<String> = roots.iter().cloned().collect();
    for edges in [&base.edges, &head.edges] {
        for edge in edges {
            nodes.insert(edge.from.clone());
            nodes.insert(edge.to.clone());
        }
    }
    nodes
}

fn tofu_owned(changed: &BTreeSet<String>, nodes: &BTreeSet<String>) -> BTreeSet<String> {
    changed
        .iter()
        .filter(|path| owns(nodes, path))
        .cloned()
        .collect()
}

fn owns(nodes: &BTreeSet<String>, path: &str) -> bool {
    let mut current = path;
    loop {
        if nodes.contains(current) {
            return true;
        }
        if let Some((parent, _)) = current.rsplit_once('/') {
            current = parent;
        } else if current.is_empty() {
            return false;
        } else {
            current = "";
        }
    }
}

pub(crate) fn derive_tofu(
    statuses: &[DetectionStatus],
    files: &[String],
) -> Result<Vec<ProposedTask>, OrchestratorError> {
    use velnor_actions_tofu::{RootLocatorRegistry, TofuTaskGroup, TofuTaskKind};
    let selected = tofu_selected_roots(statuses);
    let covered = velnor_actions_tofu::covered_fmt_roots(&selected);
    let mut locators = RootLocatorRegistry::default();
    let mut proposals = Vec::new();
    for root in &selected {
        let _locator = locators.admit(root)?;
        for kind in [
            TofuTaskKind::Fmt,
            TofuTaskKind::InitForValidate,
            TofuTaskKind::Validate,
        ] {
            let no_fmt = kind == TofuTaskKind::Fmt
                && (velnor_actions_tofu::fmt_scope_for_root(files, root).is_empty()
                    || covered.contains(root));
            let group = TofuTaskGroup {
                root: root.clone(),
                kind,
                configuration: "default".to_owned(),
                no_targets: no_fmt,
            };
            let task = velnor_actions_tofu::propose_task(&group)?;
            task.validate()?;
            proposals.push(task);
        }
    }
    Ok(proposals)
}
