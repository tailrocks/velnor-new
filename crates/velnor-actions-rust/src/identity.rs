// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "identity_closure.rs"]
mod identity_closure;

pub use self::identity_closure::{
    DependencyCoverage, DependencyCoverageInputs, UnresolvedInput, dependency_closure_ids,
    dependency_coverage, normalize_identity_path, unresolved_inputs,
};

use velnor_actions_contract::{ContractError, ProposedTask, component_id_for_unit};

pub use crate::task_identity::DigestSlot;

use crate::argv::{entry_metadata, require_nextest_for_shards, shards_allowed};
use crate::evidence::Evidence;
use crate::metadata::WorkspaceRecord;
use crate::profile::{CompileDriver, TestRunner};
use crate::resolved_graph::ResolvedGraph;
use crate::task_identity::{ExtensionInputs, RustTaskIdentityExtension};
use crate::tasks::{TaskGroup, TaskKind};

#[derive(Debug, Clone)]
pub struct GroupExtensionInputs<'a> {
    pub package_id: &'a str,
    pub workspace_id: &'a str,
    pub profile: &'a str,
    pub manifest: &'a str,
    pub graph_digest: &'a str,
    pub targets: &'a [String],
    pub config_digest: &'a str,
    pub lock_digest: DigestSlot,
    pub nextest_digest: DigestSlot,
    pub archive_source: Option<&'a str>,
    pub rerun_inputs: Option<&'a [String]>,
    pub has_build_script: bool,
}

impl TaskGroup {
    #[must_use]
    pub fn identity_extension(
        &self,
        inputs: &GroupExtensionInputs<'_>,
    ) -> RustTaskIdentityExtension {
        let package_id = component_id_for_unit(inputs.package_id, inputs.manifest);
        let derived = ExtensionInputs {
            package_id: &package_id,
            workspace_id: inputs.workspace_id,
            profile: inputs.profile,
            manifest: inputs.manifest,
            graph_digest: inputs.graph_digest,
            targets: inputs.targets,
            features: &self.features,
            all_features: self.all_features,
            target: &self.target,
            driver: self.compile_driver,
            runner: self.test_runner,
            config_digest: inputs.config_digest,
            lock_digest: inputs.lock_digest.clone(),
            nextest_digest: inputs.nextest_digest.clone(),
            kind: self.kind,
            archive_source: inputs.archive_source,
            rerun_inputs: inputs.rerun_inputs,
            has_build_script: inputs.has_build_script,
            declared_inputs: &self.declared_inputs,
        };
        RustTaskIdentityExtension::for_task(&derived)
    }

    pub fn with_rerun_inputs(self, rerun: &[String]) -> Result<Self, ContractError> {
        let mut merged = self.declared_inputs.clone();
        merged.extend(rerun.iter().cloned());
        self.with_declared_inputs(&merged)
    }

    #[must_use]
    pub fn consumes(&self, path: &str) -> bool {
        self.declared_inputs.iter().any(|input| input == path)
    }
}

impl RustTaskIdentityExtension {
    pub fn dependency_coverage(
        &self,
        package_id: &str,
        graph: &ResolvedGraph,
        workspaces: &[WorkspaceRecord],
        native_inputs: &[String],
    ) -> Result<DependencyCoverage, ContractError> {
        dependency_coverage(&DependencyCoverageInputs {
            package_id,
            graph,
            workspaces,
            features: &self.features,
            all_features: self.all_features,
            targets: &self.targets,
            lock_digest: &self.lock_slot,
            native_inputs,
        })
    }

    #[must_use]
    pub fn conservative_execution_required(&self) -> bool {
        self.undeclared_reads || self.first_blocking_input().is_some()
    }

    #[must_use]
    pub fn archive_trust_source(&self) -> Option<&str> {
        self.archive.as_deref()
    }
}

pub fn expand_shards_for_group(
    kind: TaskKind,
    runner: TestRunner,
    count: u32,
    no_test_targets: bool,
) -> Result<bool, ContractError> {
    require_nextest_for_shards(runner, count)?;
    Ok(kind == TaskKind::Nextest && shards_allowed(runner) && count > 1 && !no_test_targets)
}

#[must_use]
pub fn adapter_entry_metadata(group: &TaskGroup, evidence: &[Evidence]) -> serde_json::Value {
    let meta = entry_metadata(group, evidence);
    entry_metadata_json(
        &group.package_id,
        &group.package_name,
        &group.manifest_key,
        group.kind.as_str(),
        &group.configuration,
        &group.target,
        &meta,
    )
}

pub fn entry_metadata_for_task(
    task: &ProposedTask,
    evidence: &[Evidence],
) -> Result<serde_json::Value, ContractError> {
    let meta = crate::tasks::EntryMetadata {
        compile_driver: CompileDriver::parse(&task.identity.compile_driver)?,
        test_runner: TestRunner::parse(&task.identity.test_runner)?,
        evidence_ids: evidence.iter().map(crate::tasks::evidence_id).collect(),
    };
    Ok(entry_metadata_json(
        &task.identity.unit_id,
        &task.display_name,
        &task.identity.unit_key,
        &task.task_kind,
        &task.configuration,
        &task.identity.target,
        &meta,
    ))
}

fn entry_metadata_json(
    package_id: &str,
    package_name: &str,
    manifest_key: &str,
    kind: &str,
    configuration: &str,
    target: &str,
    meta: &crate::tasks::EntryMetadata,
) -> serde_json::Value {
    serde_json::json!({
        "package_id": package_id,
        "package_name": package_name,
        "manifest_key": manifest_key,
        "kind": kind,
        "configuration": configuration,
        "target": target,
        "compile_driver": meta.compile_driver.as_str(),
        "test_runner": meta.test_runner.as_str(),
        "evidence_ids": meta.evidence_ids,
    })
}

pub fn extension_for_proposal(
    task: &ProposedTask,
    inputs: &GroupExtensionInputs<'_>,
) -> Result<RustTaskIdentityExtension, ContractError> {
    let package_id = component_id_for_unit(inputs.package_id, inputs.manifest);
    let identity = &task.identity;
    let derived = ExtensionInputs {
        package_id: &package_id,
        workspace_id: inputs.workspace_id,
        profile: inputs.profile,
        manifest: inputs.manifest,
        graph_digest: inputs.graph_digest,
        targets: inputs.targets,
        features: &identity.features,
        all_features: identity.flags.iter().any(|flag| flag == "--all-features"),
        target: &identity.target,
        driver: CompileDriver::parse(&identity.compile_driver)?,
        runner: TestRunner::parse(&identity.test_runner)?,
        config_digest: inputs.config_digest,
        lock_digest: inputs.lock_digest.clone(),
        nextest_digest: inputs.nextest_digest.clone(),
        kind: TaskKind::parse(&task.task_kind)?,
        archive_source: inputs.archive_source,
        rerun_inputs: inputs.rerun_inputs,
        has_build_script: inputs.has_build_script,
        declared_inputs: &identity.declared_inputs,
    };
    Ok(RustTaskIdentityExtension::for_task(&derived))
}
