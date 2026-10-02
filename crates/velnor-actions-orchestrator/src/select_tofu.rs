//! Tofu calling-root selection over base/head module graphs.
//!
//! The head graph is captured at inventory time (qualified per-root
//! edges); the base graph is built from base-revision texts of
//! head-unit files minus added files plus deleted config files.
//! Changed paths split by tofu ownership: tofu-owned paths select
//! through [`select_roots`](velnor_actions_tofu::select_roots) over
//! the union, rust-owned paths keep the package path. Graph-build
//! failures broaden via the caller, like the rust base edges.

use std::collections::BTreeSet;
use std::path::Path;

use velnor_actions_contract::{DetectionStatus, ProposedTask};
use velnor_actions_tofu::{
    Family, ModuleEdges, chdir_finding_for_root, family_of, key_for_root, root_for_key,
    select_roots,
};

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::select_edges::{added_files, base_manifests, deleted_files};

#[cfg(test)]
#[path = "select_tofu_tests.rs"]
mod select_tofu_tests;

/// One tofu unit's selection record: head files plus qualified edges.
#[derive(Debug, Clone)]
pub struct TofuSelectionUnit {
    /// Normalized configured root.
    pub root: String,
    /// Config-family unit files at head (base-text candidates).
    pub files: Vec<String>,
    /// Qualified head module edges for this root.
    pub edges: ModuleEdges,
}

/// Max base files fetched; beyond this, broaden instead of reading.
const MAX_TOFU_BASE_FILE_BATCH: usize = 512;

/// Normalized configured roots backing selected tofu projects.
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

/// `path.cwd` caveats for tofu subdir roots, one per root.
///
/// Subdir-root payloads run under `-chdir`, so each configured
/// subdir root records its caveat finding; the repo root needs none
/// (its identity carries `.`). Roots derive from the proposals and
/// sort for a stable warning order.
pub(crate) fn push_chdir_findings(discovery: &Discovery, warnings: &mut Vec<String>) {
    let mut findings: BTreeSet<String> = BTreeSet::new();
    for task in &discovery.proposals {
        if task.stack_id != velnor_actions_tofu::STACK_ID {
            continue;
        }
        let root = root_for_key(&task.identity.unit_key);
        if let Some(finding) = chdir_finding_for_root(&root) {
            findings.insert(finding);
        }
    }
    warnings.extend(findings);
}

/// Split `changed` by tofu ownership and select affected tofu keys.
///
/// Returns the rust-owned subset plus the affected tofu unit keys.
/// Without tofu roots this is a no-op passthrough (no git calls).
/// Fallback reasons record as warnings; graph-build failures are
/// errors that broaden via the caller.
///
/// Base roots equal head roots: configured-root deletions fall back
/// to select-all conservatively instead of parsing base config.
pub(crate) fn split_and_select(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Result<(BTreeSet<String>, BTreeSet<String>), String> {
    let head_roots = tofu_selected_roots(&discovery.statuses);
    if head_roots.is_empty() {
        return Ok((changed.clone(), BTreeSet::new()));
    }
    let head_graph = head_module_graph(discovery);
    let base_graph = base_module_graph(root, base, head, discovery)?;
    let nodes = union_nodes(&head_roots, &base_graph, &head_graph);
    let (rust_changed, tofu_changed) = split_changed(changed, &nodes);
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
    let affected = selection
        .selected
        .into_iter()
        .map(|root| key_for_root(&root))
        .collect();
    Ok((rust_changed, affected))
}

/// Split and select, recording graph-build failures as broaden warnings.
///
/// Returns `None` when selection is unavailable (the caller broadens).
pub(crate) fn split_or_broaden(
    root: &Path,
    base: &str,
    head: &str,
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    warnings: &mut Vec<String>,
) -> Option<(BTreeSet<String>, BTreeSet<String>)> {
    match split_and_select(root, base, head, discovery, changed, warnings) {
        Ok(split) => Some(split),
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:all_changed"));
            None
        }
    }
}

/// Merged head module graph over every selection record.
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

/// Base module graph from base texts of head files plus deleted configs.
///
/// Head-unit config files minus added files plus deleted config
/// files cover every base-existing unit file; shadowing is ignored
/// (extra base edges only widen selection). References parse without
/// validation (history predates checks); any failure broadens.
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

/// Union attribution nodes: roots plus both graphs' edge ends.
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

/// Split `changed` into rust-owned and tofu-owned paths.
///
/// A path is tofu-owned when its nearest ancestor-or-self node is a
/// tofu node (mirrors selection's attribution, so every tofu-owned
/// path attributes inside [`select_roots`](velnor_actions_tofu::select_roots)).
fn split_changed(
    changed: &BTreeSet<String>,
    nodes: &BTreeSet<String>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut rust = BTreeSet::new();
    let mut tofu = BTreeSet::new();
    for path in changed {
        if owns(nodes, path) {
            tofu.insert(path.clone());
        } else {
            rust.insert(path.clone());
        }
    }
    (rust, tofu)
}

/// True when `path` has an ancestor-or-self node in `nodes`.
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

/// Derive one fmt/init/validate triple per selected tofu root.
///
/// Derivation is joint over the whole root set: a root's fmt leg
/// carries `no_targets` when its scope is empty or when another
/// selected root's fmt leg already covers it (nested scopes merge so
/// every file formats exactly once). Init and validate always derive
/// exactly once per root.
///
/// # Errors
///
/// Returns contract errors when a triple falls outside the task-ID
/// grammar or a proposal fails validation.
pub(crate) fn derive_tofu(
    statuses: &[DetectionStatus],
    files: &[String],
) -> Result<Vec<ProposedTask>, OrchestratorError> {
    use velnor_actions_tofu::{TofuTaskGroup, TofuTaskKind};
    let selected = tofu_selected_roots(statuses);
    let covered = velnor_actions_tofu::covered_fmt_roots(&selected);
    let mut proposals = Vec::new();
    for root in &selected {
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
