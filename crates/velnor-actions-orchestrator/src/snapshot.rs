// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Serialize;
use velnor_actions_contract::{
    ContractError, PlannedPlatform, ProposedTask, Stack, canonical_json_bytes, digest_b3,
};

pub(crate) const UNKNOWN_ARCHIVE_SOURCE: &str = "velnor-unknown-archive-source-v1";
pub(crate) const UNRESOLVED_GENERATOR_SHA: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
pub(crate) fn canonical_digest<T: Serialize>(value: &T) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(value)?))
}

pub(crate) fn map_release_triple(arch: &str, os: &str) -> String {
    match (arch, os) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        _ => return format!("{arch}-{os}"),
    }
    .to_owned()
}

pub(crate) fn platform_id_for(label: &str, target: &str) -> Result<String, ContractError> {
    Ok(PlannedPlatform::new(label, target)?.platform_id)
}

pub(crate) fn platform_target_for_group(
    label: &str,
    task: &ProposedTask,
) -> Result<String, ContractError> {
    let stack = Stack::require_known(&task.stack_id)?;
    if stack == Stack::Mise {
        let label = task.runner_profile.as_str();
        if let Some(host) = velnor_actions_contract::ReleaseTarget::for_runner_label(label)
            .map(velnor_actions_contract::ReleaseTarget::triple)
            && host != task.identity.target
        {
            return Err(ContractError::identity(
                "runner_label",
                "runner_target_mismatch",
            ));
        }
        return Ok(task.identity.target.clone());
    }
    let host = velnor_actions_contract::ReleaseTarget::for_runner_label(label)
        .filter(|target| *target == velnor_actions_contract::ReleaseTarget::LinuxX86_64)
        .map(velnor_actions_contract::ReleaseTarget::triple)
        .ok_or_else(|| {
            ContractError::identity(
                "runner_label",
                format!("unsupported_target_for_runner:{label}"),
            )
        })?;
    let target = if task.identity.target == "host" {
        host.to_owned()
    } else {
        task.identity.target.clone()
    };
    if target != host {
        if !velnor_actions_contract::is_supported_target(&target) {
            return Err(ContractError::identity(
                "target",
                format!("unsupported_target:{target}"),
            ));
        }
        return Err(ContractError::identity("target", "runner_target_mismatch"));
    }
    Ok(target)
}

#[derive(Debug, Clone)]
pub(crate) struct ExecutionSnapshot {
    graph_digests: std::collections::BTreeMap<String, String>,
    workspace_ids: std::collections::BTreeMap<String, String>,
    member_index: std::collections::BTreeMap<String, String>,
    manifest_index: std::collections::BTreeMap<String, String>,
    config_digest: String,
    package_meta: std::collections::BTreeMap<String, (Vec<String>, bool)>,
    rust_cache: std::cell::RefCell<velnor_actions_rust::RustClosureCache>,
}

impl ExecutionSnapshot {
    pub(crate) fn build(discovery: &crate::discover::Discovery) -> Self {
        let mut graph_digests = std::collections::BTreeMap::new();
        let mut workspace_ids = std::collections::BTreeMap::new();
        let mut member_index = std::collections::BTreeMap::new();
        let mut manifest_index = std::collections::BTreeMap::new();
        let mut package_meta = std::collections::BTreeMap::new();
        for workspace in &discovery.workspaces {
            let graph = super::identities::snapshot_graph_for(&workspace.record);
            let digest = canonical_digest(&graph).unwrap_or_else(|_| digest_b3(b"graph_error"));
            graph_digests.insert(workspace.record.workspace_root.clone(), digest.clone());
            workspace_ids.insert(workspace.record.workspace_root.clone(), digest);
            for package in &workspace.record.packages {
                member_index.insert(package.id.clone(), workspace.record.workspace_root.clone());
                manifest_index.insert(package.manifest.clone(), package.id.clone());
                let targets = package
                    .targets
                    .iter()
                    .map(|target| format!("{}:{}", target.kind, target.name))
                    .collect();
                package_meta.insert(package.id.clone(), (targets, package.has_build_script));
            }
        }
        Self {
            graph_digests,
            workspace_ids,
            member_index,
            manifest_index,
            config_digest: super::identities::tool_config_digest(&discovery.tool_checks),
            package_meta,
            rust_cache: std::cell::RefCell::new(velnor_actions_rust::RustClosureCache::new()),
        }
    }

    pub(crate) fn config_digest(&self) -> &str {
        &self.config_digest
    }

    pub(crate) fn package_meta_for(
        &self,
        package_id: &str,
        manifest: &str,
    ) -> Option<&(Vec<String>, bool)> {
        if let Some(meta) = self.package_meta.get(package_id) {
            return Some(meta);
        }
        if package_id.is_empty() {
            return self
                .manifest_index
                .get(manifest)
                .and_then(|id| self.package_meta.get(id));
        }
        None
    }

    pub(crate) fn rust_lock_digest(
        &self,
        root: &std::path::Path,
        manifest: &str,
    ) -> velnor_actions_rust::tasks::DigestSlot {
        velnor_actions_rust::lock_digest_cached(root, manifest, &mut self.rust_cache.borrow_mut())
    }

    pub(crate) fn rust_nextest_digest(
        &self,
        root: &std::path::Path,
        profile_config: Option<&str>,
    ) -> velnor_actions_rust::tasks::DigestSlot {
        velnor_actions_rust::nextest_digest_cached(
            root,
            profile_config,
            &mut self.rust_cache.borrow_mut(),
        )
    }

    pub(crate) fn resolve_rust_closure(
        &self,
        root: &std::path::Path,
        task: &ProposedTask,
        profile_nextest_config: Option<&str>,
        graph_digest: &str,
        toolchain_id: &str,
        platform_id: &str,
    ) -> Result<velnor_actions_contract::TaskInputClosure, ContractError> {
        velnor_actions_rust::resolve_closure_at_root_cached(
            root,
            task,
            profile_nextest_config,
            graph_digest,
            toolchain_id,
            platform_id,
            &mut self.rust_cache.borrow_mut(),
        )
    }

    pub(crate) fn graph_digest_for(&self, package_id: &str, manifest: &str) -> String {
        self.owner_root(package_id, manifest)
            .and_then(|root| self.graph_digests.get(root))
            .cloned()
            .unwrap_or_else(|| digest_b3(b"no-workspace"))
    }

    pub(crate) fn workspace_id_for(&self, package_id: &str, manifest: &str) -> String {
        self.owner_root(package_id, manifest)
            .and_then(|root| self.workspace_ids.get(root))
            .cloned()
            .unwrap_or_else(|| digest_b3(b"no-workspace"))
    }

    fn owner_root(&self, package_id: &str, manifest: &str) -> Option<&String> {
        if let Some(root) = self.member_index.get(package_id) {
            return Some(root);
        }
        self.manifest_index
            .get(manifest)
            .and_then(|id| self.member_index.get(id))
    }
}
