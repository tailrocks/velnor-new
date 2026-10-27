// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{NativeAuthorityError, NativeAuthorityInput, invalid};
use std::collections::BTreeSet;
use velnor_actions_contract::config::CheckRunner;

#[derive(Debug, Clone)]
pub struct CargoWorkspaceFact {
    pub root: String,
    pub lockfile_path: Option<String>,
    pub test_runner: velnor_actions_rust::TestRunner,
}
#[derive(Debug, Clone)]
pub struct RustTaskFact {
    pub unit: String,
    pub no_targets: bool,
    pub manifest: String,
    pub kind: velnor_actions_rust::TaskKind,
    pub configuration: String,
    pub all_features: bool,
}
#[derive(Debug, Clone)]
pub struct WorkspaceFormatFact {
    pub manifest: String,
    pub configuration: String,
    pub target: String,
    pub compile_driver: velnor_actions_rust::CompileDriver,
}
#[derive(Debug, Clone)]
pub struct TofuTaskFact {
    pub root: String,
    pub kind: velnor_actions_tofu::TofuTaskKind,
    pub configuration: String,
}
#[derive(Debug, Clone)]
pub struct NativeTaskFact {
    pub compile_driver: Option<velnor_actions_rust::CompileDriver>,
    pub unit: String,
    pub display_name: String,
    pub configuration: String,
    pub rust_manifest: Option<String>,
    pub tofu: Vec<TofuTaskFact>,
}
#[derive(Debug, Clone)]
pub struct GradleRootFact {
    pub root: String,
    pub projects: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedWorkflowKind {
    BunMember,
    BunSuite,
    BunAssets,
    Dbt,
    Ansible,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuitePhase {
    Build,
    Run,
}
#[derive(Debug, Clone)]
pub enum LogicalJobSource {
    Task(NativeTaskFact),
    NamedCheck {
        id: String,
    },
    Gradle {
        root: String,
        project: String,
    },
    Suite {
        id: String,
        phase: SuitePhase,
    },
    WorkflowTask {
        owner: String,
    },
    GeneratedWorkflowTask {
        owner: String,
        kind: GeneratedWorkflowKind,
    },
    DependencySeed {
        root: String,
    },
}
#[derive(Debug, Clone)]
pub struct LogicalJobFact {
    pub id: String,
    pub runs_on: String,
    pub check_runner: Option<CheckRunner>,
    pub needs: Vec<String>,
    /// Declared job outputs (`jobs.<id>.outputs`), mirrored from the built
    /// job so the authority projection pins the same data-flow interface.
    pub outputs: std::collections::BTreeMap<String, String>,
    pub source: LogicalJobSource,
}

pub(super) fn fetch_roots(
    workspaces: &[CargoWorkspaceFact],
) -> Result<Vec<String>, NativeAuthorityError> {
    let mut roots = BTreeSet::new();
    let mut fetch = BTreeSet::new();
    for workspace in workspaces {
        validate_root(&workspace.root)?;
        if !roots.insert(&workspace.root) {
            return Err(invalid("native_workspace_duplicate"));
        }
        if let Some(lock) = &workspace.lockfile_path {
            let expected = if workspace.root.is_empty() {
                "Cargo.lock".to_owned()
            } else {
                format!("{}/Cargo.lock", workspace.root)
            };
            if *lock != expected {
                return Err(invalid("native_workspace_lock_path"));
            }
            fetch.insert(workspace.root.clone());
        }
    }
    Ok(fetch.into_iter().collect())
}

pub(super) fn validate_root(root: &str) -> Result<(), NativeAuthorityError> {
    velnor_actions_contract::validate_fetch_root(root)?;
    if root.chars().any(char::is_control)
        || !root.is_empty() && root.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(invalid("native_workspace_root"));
    }
    Ok(())
}

pub(super) fn admit_manifest(
    manifest: &str,
    workspaces: &[CargoWorkspaceFact],
    exact: bool,
) -> Result<(), NativeAuthorityError> {
    let root = manifest
        .strip_suffix("Cargo.toml")
        .and_then(|prefix| {
            if prefix.is_empty() {
                Some("")
            } else {
                prefix.strip_suffix('/')
            }
        })
        .ok_or_else(|| invalid("native_rust_manifest"))?;
    validate_root(root)?;
    let admitted = workspaces
        .iter()
        .filter(|workspace| {
            root == workspace.root
                || !exact
                    && (workspace.root.is_empty()
                        || root.starts_with(&format!("{}/", workspace.root)))
        })
        .max_by_key(|workspace| workspace.root.len());
    if admitted.is_none() {
        return Err(invalid("native_rust_manifest_workspace"));
    }
    Ok(())
}

pub(super) mod generated;
mod graph;
mod registered;
mod seeds;
mod task_ids;
mod tool_needs;

pub use generated::GeneratedSourceFact;
pub(super) use graph::validate_graph;
pub use seeds::unique_dependency_seed_job_ids;
