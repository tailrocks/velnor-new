//! Selection-time identity attachment, coverage, archives, and sharding.
//!
//! Pure helpers the orchestrator calls between derivation and selection so
//! every Rust node carries its [`RustTaskIdentityExtension`] before selection
//! or reuse is considered (par §3). This module launches no processes, reads
//! no files, and builds no tool invocations.

use velnor_actions_contract::ContractError;

use crate::argv::{ExtensionInputs, shards_allowed};
use crate::argv::{RustTaskIdentityExtension, entry_metadata, require_nextest_for_shards};
use crate::evidence::Evidence;
use crate::tasks::{TaskGroup, TaskKind};

/// Digests and build facts the orchestrator supplies per extension.
#[derive(Debug, Clone)]
pub struct GroupExtensionInputs<'a> {
    /// Cargo package ID.
    pub package_id: &'a str,
    /// Normalized manifest path.
    pub manifest: &'a str,
    /// Workspace/local-package graph digest.
    pub graph_digest: &'a str,
    /// Target kinds and names.
    pub targets: &'a [String],
    /// Cargo config and build-script input digests.
    pub config_digest: &'a str,
    /// `Cargo.lock` digest, when the lockfile is available.
    pub lock_digest: Option<&'a str>,
    /// `.config/nextest.toml` digest for Nextest profiles.
    pub nextest_digest: Option<&'a str>,
    /// Build task id producing the archive (Nextest `Build` only).
    pub archive_source: Option<&'a str>,
    /// Declared `rerun-if-changed` inputs (`None` means unknown).
    pub rerun_inputs: Option<&'a [String]>,
    /// Whether the package carries a build script.
    pub has_build_script: bool,
}

impl TaskGroup {
    /// Derive this group's identity extension before selection and reuse.
    ///
    /// Group-owned fields (kind, features, target, driver, runner, declared
    /// inputs) come from `self`; workspace digests come from `inputs`. The
    /// orchestrator MUST call this for every group and feed the result into
    /// the input-digest preimage plus the reuse gate.
    #[must_use]
    pub fn identity_extension(
        &self,
        inputs: &GroupExtensionInputs<'_>,
    ) -> RustTaskIdentityExtension {
        let derived = ExtensionInputs {
            package_id: inputs.package_id,
            manifest: inputs.manifest,
            graph_digest: inputs.graph_digest,
            targets: inputs.targets,
            features: &self.features,
            target: &self.target,
            driver: &self.compile_driver,
            runner: &self.test_runner,
            config_digest: inputs.config_digest,
            lock_digest: inputs.lock_digest,
            nextest_digest: inputs.nextest_digest,
            kind: self.kind,
            archive_source: inputs.archive_source,
            rerun_inputs: inputs.rerun_inputs,
            has_build_script: inputs.has_build_script,
            declared_inputs: &self.declared_inputs,
        };
        RustTaskIdentityExtension::for_task(&derived)
    }

    /// Declare build-script `rerun-if-changed` paths as task inputs.
    ///
    /// Producer for par §1: non-Rust files (schemas, fixtures, native build
    /// inputs) become declared inputs of the Rust task. Feed this with
    /// [`parse_rerun_changed`](crate::argv::parse_rerun_changed) output.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for empty, absolute, or traversing paths.
    pub fn with_rerun_inputs(self, rerun: &[String]) -> Result<Self, ContractError> {
        let mut merged = self.declared_inputs.clone();
        merged.extend(rerun.iter().cloned());
        self.with_declared_inputs(&merged)
    }
}

impl RustTaskIdentityExtension {
    /// Reject baseline coverage when build inputs are undeclared or dynamic.
    ///
    /// Mirrors [`reuse_eligible`](Self::reuse_eligible): par §4.2 disables
    /// both exact reuse and baseline coverage for undeclared reads.
    /// # Errors
    pub fn coverage_eligible(&self) -> Result<(), ContractError> {
        if self.undeclared_reads {
            return Err(ContractError::identity(
                "stack_extension",
                "undeclared_inputs_no_coverage",
            ));
        }
        Ok(())
    }

    /// Whether the package MUST execute conservatively (no reuse, no cover).
    #[must_use]
    pub fn conservative_execution_required(&self) -> bool {
        self.undeclared_reads
    }

    /// Trust and retention anchor of this task's archive, if it has one.
    ///
    /// An archive is compiled executable data, so trust and retention rules
    /// match its source (par §8): the returned build task id names the task
    /// whose trust level and retention apply. `None` means no archive and
    /// nothing inherited (every cargo-test profile, every non-`Build` kind).
    #[must_use]
    pub fn archive_trust_source(&self) -> Option<&str> {
        self.archive.as_deref()
    }
}

/// Whether `kind` expands into per-shard groups under `runner`.
///
/// The orchestrator's shard expansion MUST call this instead of matching on
/// kinds itself: only [`TaskKind::Nextest`] ever expands, cargo-test counts
/// above one are rejected, and target-less groups never shard.
///
/// # Errors
///
/// Returns [`ContractError`] when `count` exceeds one for cargo-test.
pub fn expand_shards_for_group(
    kind: TaskKind,
    runner: &str,
    count: u32,
    no_test_targets: bool,
) -> Result<bool, ContractError> {
    require_nextest_for_shards(runner, count)?;
    Ok(kind == TaskKind::Nextest && shards_allowed(runner) && count > 1 && !no_test_targets)
}

/// Full adapter entry metadata: legacy fields plus driver/runner/evidence.
///
/// Entries MUST carry the detected `compile_driver`/`test_runner` plus
/// evidence ids (wf §4) alongside the package/kind/configuration/target
/// fields the orchestrator already emits.
#[must_use]
pub fn adapter_entry_metadata(group: &TaskGroup, evidence: &[Evidence]) -> serde_json::Value {
    let meta = entry_metadata(group, evidence);
    serde_json::json!({
        "package_id": group.package_id,
        "package_name": group.package_name,
        "manifest_key": group.manifest_key,
        "kind": group.kind.as_str(),
        "configuration": group.configuration,
        "target": group.target,
        "compile_driver": meta.compile_driver,
        "test_runner": meta.test_runner,
        "evidence_ids": meta.evidence_ids,
    })
}
