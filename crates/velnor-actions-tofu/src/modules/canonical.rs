//! Filesystem boundary: H5 canonicalization, acyclicity, qualification.
//!
//! Both edge ends realpath against the checkout and must stay
//! in-repo; every target must hold an effective config file. Cycles
//! are errors. Findings pass through untouched (selection
//! fallbacks, never errors).

use std::collections::BTreeMap;
use std::path::Path;

use crate::effective::effective_in_dir;

use super::grammar::{ModuleError, ModuleRef};
use super::resolve::{ModuleEdge, ModuleEdges, resolve_refs};

/// Canonicalize both ends of every edge at the filesystem boundary (H5).
///
/// Both ends realpath against `root` and must stay in-repo; every
/// target must hold an effective config file among `files` (the repo
/// file index). Returns edges with canonical repo-relative ends.
///
/// # Errors
///
/// Returns [`ModuleError`] for escapes, missing or unreadable
/// targets, and an unreadable repository root.
pub fn canonicalize_edges(
    root: &Path,
    files: &[String],
    edges: &[ModuleEdge],
) -> Result<Vec<ModuleEdge>, ModuleError> {
    let canonical = root.canonicalize().map_err(|_| ModuleError::Unreadable {
        target: String::new(),
    })?;
    let mut canonical_edges = Vec::with_capacity(edges.len());
    for edge in edges {
        let from = canonicalize_side(&canonical, &edge.from)?;
        let to = canonicalize_side(&canonical, &edge.to)?;
        if effective_in_dir(files, &to).is_empty() {
            return Err(ModuleError::MissingTarget {
                target: edge.to.clone(),
            });
        }
        canonical_edges.push(ModuleEdge {
            from,
            to,
            source: edge.source.clone(),
        });
    }
    canonical_edges.sort_by(|left, right| (&left.from, &left.to).cmp(&(&right.from, &right.to)));
    canonical_edges.dedup_by(|curr, prev| curr.from == prev.from && curr.to == prev.to);
    Ok(canonical_edges)
}

/// Realpath one edge side; in-repo sides return repo-relative POSIX.
pub(crate) fn canonicalize_side(canonical_root: &Path, side: &str) -> Result<String, ModuleError> {
    let resolved = canonical_root.join(side).canonicalize().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ModuleError::MissingTarget {
                target: side.to_owned(),
            }
        } else {
            ModuleError::Unreadable {
                target: side.to_owned(),
            }
        }
    })?;
    let relative = resolved
        .strip_prefix(canonical_root)
        .map_err(|_| ModuleError::Escape {
            target: side.to_owned(),
        })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        parts.push(
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| ModuleError::Unreadable {
                    target: side.to_owned(),
                })?,
        );
    }
    Ok(parts.join("/"))
}

/// Reject directed cycles over edge ends (iterative DFS, sorted order).
///
/// # Errors
///
/// Returns [`ModuleError::Cycle`] naming the first cycle found.
pub fn check_acyclic(edges: &[ModuleEdge]) -> Result<(), ModuleError> {
    let mut adjacency: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for edge in edges {
        adjacency
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
    }
    for targets in adjacency.values_mut() {
        targets.sort_unstable();
        targets.dedup();
    }
    let mut marks: BTreeMap<&str, Mark> = BTreeMap::new();
    let mut starts: Vec<&str> = adjacency.keys().copied().collect();
    starts.sort_unstable();
    for start in starts {
        if marks.contains_key(start) {
            continue;
        }
        visit(start, &adjacency, &mut marks)?;
    }
    Ok(())
}

/// DFS mark per node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// On the current trail (re-entry is a cycle).
    Entered,
    /// Fully explored.
    Done,
}

/// Iterative DFS from `start`; re-entering the trail is a cycle.
fn visit<'edges>(
    start: &'edges str,
    adjacency: &BTreeMap<&'edges str, Vec<&'edges str>>,
    marks: &mut BTreeMap<&'edges str, Mark>,
) -> Result<(), ModuleError> {
    let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
    let mut trail: Vec<&str> = vec![start];
    marks.insert(start, Mark::Entered);
    while let Some((node, next)) = stack.pop() {
        let neighbors: &[&str] = adjacency.get(node).map_or(&[], Vec::as_slice);
        if next >= neighbors.len() {
            marks.insert(node, Mark::Done);
            trail.pop();
            continue;
        }
        stack.push((node, next + 1));
        let target = neighbors[next];
        match marks.get(target) {
            Some(Mark::Entered) => {
                let mut cycle: Vec<String> = trail
                    .iter()
                    .skip_while(|name| **name != target)
                    .map(|name| (*name).to_owned())
                    .collect();
                cycle.push(target.to_owned());
                return Err(ModuleError::Cycle { path: cycle });
            }
            Some(Mark::Done) => {}
            None => {
                marks.insert(target, Mark::Entered);
                trail.push(target);
                stack.push((target, 0));
            }
        }
    }
    Ok(())
}

/// Full boundary qualification: resolve, canonicalize (H5), require acyclic.
///
/// Findings pass through (selection fallbacks, never errors).
///
/// # Errors
///
/// Returns [`ModuleError`] for escapes, missing or unreadable
/// targets, and cycles.
pub fn qualify_module_edges(
    root: &Path,
    files: &[String],
    refs: &[ModuleRef],
) -> Result<ModuleEdges, ModuleError> {
    let resolved = resolve_refs(refs)?;
    let edges = canonicalize_edges(root, files, &resolved.edges)?;
    check_acyclic(&edges)?;
    Ok(ModuleEdges {
        edges,
        findings: resolved.findings,
    })
}
