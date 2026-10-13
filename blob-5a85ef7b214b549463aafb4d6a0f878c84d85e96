//! Selection-time identity attachment, coverage, archives, and sharding.
//!
//! Pure helpers the orchestrator calls between derivation and selection so
//! every Rust node carries its [`RustTaskIdentityExtension`] before selection
//! or reuse is considered (par §3). This module launches no processes, reads
//! no files, and builds no tool invocations.

// P03 resolution states live here so no `lib.rs` edit can collide.
#[path = "identity_closure.rs"]
mod identity_closure;

pub use self::identity_closure::{UnresolvedInput, normalize_identity_path, unresolved_inputs};

use velnor_actions_contract::{ContractError, ProposedTask, component_id_for_unit};

use crate::argv::{entry_metadata, require_nextest_for_shards, shards_allowed};
use crate::evidence::Evidence;
use crate::profile::{CompileDriver, TestRunner};
use crate::task_identity::{DigestSlot, ExtensionInputs, RustTaskIdentityExtension};
use crate::tasks::{TaskGroup, TaskKind};

/// Digests and build facts the orchestrator supplies per extension.
#[derive(Debug, Clone)]
pub struct GroupExtensionInputs<'a> {
    /// Cargo package ID.
    pub package_id: &'a str,
    /// Workspace identity digest.
    pub workspace_id: &'a str,
    /// Execution profile (configuration) name.
    pub profile: &'a str,
    /// Normalized manifest path.
    pub manifest: &'a str,
    /// Workspace/local-package graph digest.
    pub graph_digest: &'a str,
    /// Target kinds and names.
    pub targets: &'a [String],
    /// Cargo config and build-script input digests.
    pub config_digest: &'a str,
    /// `Cargo.lock` slot, resolved against the checkout.
    pub lock_digest: DigestSlot,
    /// `.config/nextest.toml` slot for Nextest profiles.
    pub nextest_digest: DigestSlot,
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
        let package_id = component_id_for_unit(inputs.package_id, inputs.manifest);
        let derived = ExtensionInputs {
            package_id: &package_id,
            workspace_id: inputs.workspace_id,
            profile: inputs.profile,
            manifest: inputs.manifest,
            graph_digest: inputs.graph_digest,
            targets: inputs.targets,
            features: &self.features,
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

    /// Whether this group consumes `path` as a declared non-Rust input.
    ///
    /// Selection broadening consults this: a changed tool file or fixture
    /// reselects every group consuming it.
    #[must_use]
    pub fn consumes(&self, path: &str) -> bool {
        self.declared_inputs.iter().any(|input| input == path)
    }
}

impl RustTaskIdentityExtension {
    /// Reject baseline coverage when build inputs are undeclared or dynamic.
    ///
    /// Mirrors [`reuse_eligible`](Self::reuse_eligible): par §4.2 disables
    /// both exact reuse and baseline coverage for undeclared reads and
    /// unresolved slots.
    /// # Errors
    pub fn coverage_eligible(&self) -> Result<(), ContractError> {
        if self.undeclared_reads {
            return Err(ContractError::identity(
                "stack_extension",
                "undeclared_inputs_no_coverage",
            ));
        }
        if let Some(input) = self.first_blocking_input() {
            return Err(ContractError::identity(
                "stack_extension",
                format!("unresolved_input_no_coverage:{}", input.as_str()),
            ));
        }
        Ok(())
    }

    /// Whether the package MUST execute conservatively (no reuse, no cover).
    #[must_use]
    pub fn conservative_execution_required(&self) -> bool {
        self.undeclared_reads || self.first_blocking_input().is_some()
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
    runner: TestRunner,
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

/// Full adapter entry metadata for one neutral proposal.
///
/// Byte-identical to [`adapter_entry_metadata`]; driver/runner spellings
/// parse fail-closed (validated proposals always parse).
///
/// # Errors
///
/// Returns [`ContractError`] for driver/runner spellings outside the
/// known tokens.
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

/// Entry metadata JSON over scalar fields plus typed driver/runner/evidence.
///
/// Single owner of the metadata shape for group and proposal inputs.
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

/// Derive one proposal's identity extension before selection and reuse.
///
/// Proposal-owned facts (kind, features, target, drivers, declared
/// inputs) come from `identity`; workspace digests come from `inputs`.
/// Spellings parse fail-closed (validated proposals always parse).
///
/// # Errors
///
/// Returns [`ContractError`] for kind/driver/runner spellings outside
/// the known tokens.
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
