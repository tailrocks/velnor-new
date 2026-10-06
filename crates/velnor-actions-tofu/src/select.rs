//! Calling-root selection over the base/head module-graph union.
//!
//! Changed files attribute to their nearest enclosing graph node (a
//! configured root or a module edge end, either revision); the
//! neutral reverse closure over the union selects callers, and the
//! result intersects the head roots — child module dirs are never
//! auto-promoted. Removed edges cannot drop callers; unknown,
//! dynamic, and external inputs widen to ALL roots with a recorded
//! typed reason; a cyclic head graph is an error (base cycles are
//! history and never block).

use std::collections::BTreeSet;
use std::fmt;

use velnor_actions_contract::reverse_closure;

use crate::modules::{ModuleEdges, ModuleError, SourceClass, check_acyclic, module_edge_pairs};

/// Outcome of calling-root selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootSelection {
    /// Selected head roots (never module dirs, never deleted roots).
    pub selected: BTreeSet<String>,
    /// Recorded select-ALL reasons (empty for narrow selections).
    pub fallback: BTreeSet<SelectAllReason>,
}

/// Typed reason for selecting ALL roots (explicit conservative fallback).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SelectAllReason {
    /// A changed file has no enclosing graph node.
    Unknown {
        /// Unowned changed path.
        detail: String,
    },
    /// A dynamic module source hides closure content (either revision).
    Dynamic {
        /// `file:name:reason` rendering.
        detail: String,
    },
    /// An absolute-path module source hides external content (either revision).
    External {
        /// `file:name:source` rendering.
        detail: String,
    },
}

impl fmt::Display for SelectAllReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { detail } => write!(f, "unknown:{detail}"),
            Self::Dynamic { detail } => write!(f, "dynamic_source:{detail}"),
            Self::External { detail } => write!(f, "external_source:{detail}"),
        }
    }
}

/// Select calling roots for `changed` over the base/head union.
///
/// `head_roots` are the configured head roots; `base_roots` the
/// configured base roots (deleted roots attribute but never emit).
/// Findings in either revision widen to ALL head roots with recorded
/// reasons; remote findings never widen. An empty change set selects
/// nothing, even with findings.
///
/// # Errors
///
/// Returns [`ModuleError::Cycle`] when the head graph is cyclic.
pub fn select_roots(
    head_roots: &[String],
    base_roots: &[String],
    base: &ModuleEdges,
    head: &ModuleEdges,
    changed: &BTreeSet<String>,
) -> Result<RootSelection, ModuleError> {
    check_acyclic(&head.edges)?;
    if changed.is_empty() {
        return Ok(RootSelection {
            selected: BTreeSet::new(),
            fallback: BTreeSet::new(),
        });
    }
    let mut nodes: BTreeSet<&str> = BTreeSet::new();
    nodes.extend(head_roots.iter().map(String::as_str));
    nodes.extend(base_roots.iter().map(String::as_str));
    for edges in [&base.edges, &head.edges] {
        for edge in edges {
            nodes.insert(edge.from.as_str());
            nodes.insert(edge.to.as_str());
        }
    }
    let mut reasons = fallback_reasons(base, head);
    let mut owned = BTreeSet::new();
    for path in changed {
        match nearest_node(&nodes, path) {
            Some(dir) => {
                owned.insert(dir);
            }
            None => {
                reasons.insert(SelectAllReason::Unknown {
                    detail: path.clone(),
                });
            }
        }
    }
    if reasons.is_empty() {
        let pairs_base = module_edge_pairs(&base.edges);
        let pairs_head = module_edge_pairs(&head.edges);
        let affected = reverse_closure(&pairs_base, &pairs_head, &owned);
        let selected: BTreeSet<String> = affected
            .into_iter()
            .filter(|dir| head_roots.contains(dir))
            .collect();
        Ok(RootSelection {
            selected,
            fallback: reasons,
        })
    } else {
        Ok(RootSelection {
            selected: head_roots.iter().cloned().collect(),
            fallback: reasons,
        })
    }
}

/// Select-ALL reasons from both revisions' findings (remote never widens).
fn fallback_reasons(base: &ModuleEdges, head: &ModuleEdges) -> BTreeSet<SelectAllReason> {
    let mut reasons = BTreeSet::new();
    for finding in base.findings.iter().chain(head.findings.iter()) {
        let detail = format!("{}:{}:{}", finding.file, finding.name, finding.detail);
        match finding.class {
            SourceClass::Dynamic => {
                reasons.insert(SelectAllReason::Dynamic { detail });
            }
            SourceClass::External => {
                reasons.insert(SelectAllReason::External { detail });
            }
            SourceClass::Local | SourceClass::Remote(_) => {}
        }
    }
    reasons
}

/// Nearest ancestor-or-self of `path` present in `nodes`.
fn nearest_node(nodes: &BTreeSet<&str>, path: &str) -> Option<String> {
    let mut current = path;
    loop {
        if nodes.contains(current) {
            return Some(current.to_owned());
        }
        if let Some((parent, _)) = current.rsplit_once('/') {
            current = parent;
        } else if current.is_empty() {
            return None;
        } else {
            current = "";
        }
    }
}
