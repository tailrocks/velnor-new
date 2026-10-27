// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use serde::Serialize;
use velnor_actions_contract::canonical::{Digest, canonical_json_bytes, digest_b3};
use velnor_actions_contract::{ContractError, normalize_posix_path};

use super::GroupExtensionInputs;
use crate::metadata::WorkspaceRecord;
use crate::resolved_graph::ResolvedGraph;
use crate::task_identity::{DigestSlot, RustTaskIdentityExtension, SlotState};
use crate::tasks::TaskGroup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyCoverage {
    pub dependency_digest: String,
    pub workspace_digest: String,
    pub native_digest: String,
    pub coverage_digest: String,
}

#[derive(Debug, Clone)]
pub struct DependencyCoverageInputs<'a> {
    pub package_id: &'a str,
    pub graph: &'a ResolvedGraph,
    pub workspaces: &'a [WorkspaceRecord],
    pub features: &'a [String],
    pub all_features: bool,
    pub targets: &'a [String],
    pub lock_digest: &'a DigestSlot,
    pub native_inputs: &'a [String],
}

#[derive(Serialize)]
struct DependencyRecord<'a> {
    edges: Vec<(&'a str, &'a str)>,
    features: Vec<&'a str>,
    all_features: bool,
    targets: Vec<&'a str>,
    lock_state: SlotState,
    lock_digest: Option<&'a str>,
}

#[derive(Serialize)]
struct WorkspaceDependencyRecord<'a> {
    packages: Vec<WorkspaceDependencyEntry<'a>>,
}

#[derive(Serialize)]
struct WorkspaceDependencyEntry<'a> {
    id: &'a str,
    name: &'a str,
    version: &'a str,
    manifest: &'a str,
}

pub fn dependency_closure_ids(
    graph: &ResolvedGraph,
    package_id: &str,
) -> Result<BTreeSet<String>, ContractError> {
    if package_id.is_empty() {
        return Err(ContractError::identity(
            "dependency_coverage.package_id",
            "unknown_unit",
        ));
    }
    if !graph_contains(graph, package_id) {
        return Err(ContractError::identity(
            "dependency_coverage.package_id",
            "unknown_unit",
        ));
    }
    let mut closure = BTreeSet::new();
    let mut stack = vec![package_id.to_owned()];
    while let Some(current) = stack.pop() {
        if !closure.insert(current.clone()) {
            continue;
        }
        for (from, to) in &graph.edges {
            if from == &current && !closure.contains(to) {
                stack.push(to.clone());
            }
        }
    }
    Ok(closure)
}

pub fn dependency_coverage(
    inputs: &DependencyCoverageInputs<'_>,
) -> Result<DependencyCoverage, ContractError> {
    let closure = dependency_closure_ids(inputs.graph, inputs.package_id)?;
    let features = sorted_refs(inputs.features, "dependency_coverage.features")?;
    let targets = sorted_refs(inputs.targets, "dependency_coverage.targets")?;
    let native_inputs = sorted_native_inputs(inputs.native_inputs)?;
    let (lock_state, lock_digest) = lock_identity(inputs.lock_digest)?;
    let mut edges: Vec<(&str, &str)> = inputs
        .graph
        .edges
        .iter()
        .filter(|(from, to)| closure.contains(from) && closure.contains(to))
        .map(|(from, to)| (from.as_str(), to.as_str()))
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let record = DependencyRecord {
        edges,
        features,
        all_features: inputs.all_features,
        targets,
        lock_state,
        lock_digest,
    };
    let dependency_digest = digest_b3(&canonical_json_bytes(&record)?);
    let workspace_digest = digest_b3(&canonical_json_bytes(&workspace_record(
        inputs.workspaces,
        inputs.graph,
        &closure,
        inputs.package_id,
    )?)?);
    let native_digest = digest_b3(&canonical_json_bytes(&native_inputs)?);
    let coverage_digest = digest_b3(&canonical_json_bytes(&[
        &dependency_digest,
        &workspace_digest,
        &native_digest,
    ])?);
    for digest in [
        &dependency_digest,
        &workspace_digest,
        &native_digest,
        &coverage_digest,
    ] {
        Digest::parse(digest)?;
    }
    Ok(DependencyCoverage {
        dependency_digest,
        workspace_digest,
        native_digest,
        coverage_digest,
    })
}

fn graph_contains(graph: &ResolvedGraph, package_id: &str) -> bool {
    graph
        .edges
        .iter()
        .any(|(from, to)| from == package_id || to == package_id)
        || graph
            .path_packages
            .iter()
            .any(|package| package.id == package_id)
}

fn lock_identity(slot: &DigestSlot) -> Result<(SlotState, Option<&str>), ContractError> {
    match slot {
        DigestSlot::Known(digest) => {
            Digest::parse(digest).map_err(|_| {
                ContractError::identity("dependency_coverage.lockfile", "malformed_lock_digest")
            })?;
            Ok((SlotState::Known, Some(digest.as_str())))
        }
        DigestSlot::AbsentProven(_) => Ok((SlotState::AbsentProven, None)),
        DigestSlot::Unknown(_) => Err(ContractError::identity(
            "dependency_coverage.lockfile",
            "unresolved_lockfile",
        )),
    }
}

fn sorted_refs<'a>(
    values: &'a [String],
    field: &'static str,
) -> Result<Vec<&'a str>, ContractError> {
    for value in values {
        if value.is_empty() {
            return Err(ContractError::identity(field, "empty_entry"));
        }
    }
    let mut out: Vec<&str> = values.iter().map(String::as_str).collect();
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn sorted_native_inputs(values: &[String]) -> Result<Vec<&str>, ContractError> {
    for value in values {
        normalize_identity_path(value).map_err(|_| {
            ContractError::identity("dependency_coverage.native_inputs", "bad_native_input")
        })?;
    }
    let mut out: Vec<&str> = values.iter().map(String::as_str).collect();
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn workspace_record<'a>(
    workspaces: &'a [WorkspaceRecord],
    graph: &ResolvedGraph,
    closure: &BTreeSet<String>,
    package_id: &str,
) -> Result<WorkspaceDependencyRecord<'a>, ContractError> {
    let path_ids: BTreeSet<&str> = graph
        .path_packages
        .iter()
        .map(|package| package.id.as_str())
        .collect();
    let mut packages = Vec::new();
    for workspace in workspaces {
        for package in &workspace.packages {
            if package.id == package_id
                || !closure.contains(&package.id)
                || (!package.in_workspace && !path_ids.contains(package.id.as_str()))
            {
                continue;
            }
            if package.id.is_empty() || package.manifest.is_empty() {
                return Err(ContractError::identity(
                    "dependency_coverage.workspace",
                    "unknown_unit",
                ));
            }
            packages.push(WorkspaceDependencyEntry {
                id: package.id.as_str(),
                name: package.name.as_str(),
                version: package.version.as_str(),
                manifest: package.manifest.as_str(),
            });
        }
    }
    packages.sort_by(|left, right| left.id.cmp(right.id));
    packages.dedup_by(|right, left| left.id == right.id);
    Ok(WorkspaceDependencyRecord { packages })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedInput {
    Lockfile,
    NextestConfig,
    ArchiveSource,
    RerunInputs,
}

impl UnresolvedInput {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lockfile => "lockfile",
            Self::NextestConfig => "nextest_config",
            Self::ArchiveSource => "archive_source",
            Self::RerunInputs => "rerun_inputs",
        }
    }

    #[must_use]
    pub fn blocks_gates(&self) -> bool {
        !matches!(self, Self::ArchiveSource)
    }
}

impl RustTaskIdentityExtension {
    #[must_use]
    pub fn first_blocking_input(&self) -> Option<UnresolvedInput> {
        unresolved_inputs(self)
            .into_iter()
            .find(UnresolvedInput::blocks_gates)
    }
}

#[must_use]
pub fn unresolved_inputs(ext: &RustTaskIdentityExtension) -> Vec<UnresolvedInput> {
    let mut unresolved = Vec::new();
    if ext.lock_slot.is_unknown() {
        unresolved.push(UnresolvedInput::Lockfile);
    }
    if ext.task_kind == crate::tasks::TaskKind::Nextest && ext.nextest_slot.is_unknown() {
        unresolved.push(UnresolvedInput::NextestConfig);
    }
    if ext.task_kind == crate::tasks::TaskKind::Build
        && ext.test_runner == crate::profile::TestRunner::CargoNextest
        && ext.archive.is_none()
    {
        unresolved.push(UnresolvedInput::ArchiveSource);
    }
    if ext.undeclared_reads {
        unresolved.push(UnresolvedInput::RerunInputs);
    }
    unresolved
}

pub fn normalize_identity_path(path: &str) -> Result<String, ContractError> {
    if path.is_empty() {
        return Err(ContractError::identity("path", "empty_path"));
    }
    if path.contains('\0') || path.chars().any(char::is_control) {
        return Err(ContractError::identity("path", "control_characters"));
    }
    if path.contains('\\') {
        return Err(ContractError::identity("path", "backslash_separator"));
    }
    normalize_posix_path(path)
}

impl TaskGroup {
    pub fn identity_extension_verified(
        &self,
        inputs: &GroupExtensionInputs<'_>,
    ) -> Result<RustTaskIdentityExtension, ContractError> {
        normalize_identity_path(inputs.manifest)?;
        for path in self.declared_inputs.iter().chain(inputs.targets.iter()) {
            normalize_identity_path(path)?;
        }
        if let Some(rerun) = inputs.rerun_inputs {
            for path in rerun {
                normalize_identity_path(path)?;
            }
        }
        Ok(self.identity_extension(inputs))
    }
}
