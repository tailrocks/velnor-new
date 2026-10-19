//! Discovery-time inputs for affected-work selection.
//!
//! Two facts the per-PR selector needs beyond the `--no-deps` inventory:
//! the full resolved package graph per workspace (registry edges reach
//! `[patch]`ed and vendored path crates), and the declared input rules
//! (`[[stacks.rust.inputs]]`) resolved from package names to the package
//! IDs discovery found. An unknown package name is a configuration error:
//! a rule that names nothing would silently select nothing.

use std::collections::BTreeSet;

use velnor_actions_contract::RustStackConfig;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::PlannedWorkspace;
pub(crate) use crate::inventory::WorkspaceResolve;

/// Selection inputs gathered once during discovery.
#[derive(Debug, Clone, Default)]
pub struct AffectedInputs {
    /// Full-resolve graph (or the reason none exists) per planned workspace.
    pub(crate) resolves: Vec<WorkspaceResolve>,
    /// Declared input rules with package names resolved to package IDs.
    pub(crate) rules: Vec<InputRule>,
}

/// One declared input rule bound to discovered package IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputRule {
    /// Repository-relative globs.
    pub(crate) globs: Vec<String>,
    /// Every workspace-member package ID carrying a named package.
    pub(crate) package_ids: BTreeSet<String>,
    /// The named packages declare their out-of-directory inputs complete.
    pub(crate) complete: bool,
}

impl InputRule {
    /// True when `path` matches any of this rule's globs.
    pub(crate) fn matches(&self, path: &str) -> bool {
        self.globs
            .iter()
            .any(|glob| velnor_actions_contract::matches_glob(glob, path))
    }
}

/// Bind `[[stacks.rust.inputs]]` package names to discovered member IDs.
///
/// # Errors
///
/// Returns a configuration error naming the first package no planned
/// workspace contains.
pub(crate) fn resolve_input_rules(
    rust: Option<&RustStackConfig>,
    workspaces: &[PlannedWorkspace],
) -> Result<Vec<InputRule>, OrchestratorError> {
    let Some(rust) = rust else {
        return Ok(Vec::new());
    };
    let mut rules = Vec::with_capacity(rust.inputs.len());
    for rule in &rust.inputs {
        let mut package_ids = BTreeSet::new();
        for name in &rule.packages {
            let before = package_ids.len();
            for workspace in workspaces {
                for package in &workspace.record.packages {
                    if package.in_workspace && !package.external && package.name == *name {
                        package_ids.insert(package.id.clone());
                    }
                }
            }
            if package_ids.len() == before {
                return Err(OrchestratorError::config(
                    CONFIG_REL,
                    "stacks.rust.inputs.packages",
                    format!("unknown_package:{name}"),
                ));
            }
        }
        rules.push(InputRule {
            globs: rule.globs.clone(),
            package_ids,
            complete: rule.complete,
        });
    }
    Ok(rules)
}
