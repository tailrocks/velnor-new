//! Selection-time identity attachment for tofu proposals.
//!
//! Pure helpers the orchestrator calls between derivation and selection
//! so every tofu node carries its [`TofuTaskIdentityExtension`] before
//! selection or reuse is considered. This module launches no processes
//! and builds no tool invocations; only [`lock_slot_at_root`] reads the
//! checkout (one bounded lockfile probe).

use std::path::Path;

use velnor_actions_contract::{ContractError, ProposedTask, component_id_for_unit};

use crate::file_cache::FileCache;
use crate::kinds::TofuTaskKind;
use crate::lockfile::lock_digest_at_root;
use crate::propose::{TOFU_DRIVER, TOFU_RUNNER};
use crate::task_identity::{DigestSlot, ExtensionInputs, TofuTaskIdentityExtension};

/// Digests and root facts the orchestrator supplies per extension.
#[derive(Debug, Clone)]
pub struct TofuGroupExtensionInputs<'a> {
    /// Exact `OpenTofu` root key: `dir-` plus lowercase UTF-8 hex.
    pub unit_id: &'a str,
    /// Workspace identity digest.
    pub workspace_id: &'a str,
    /// Execution profile (configuration) name.
    pub profile: &'a str,
    /// Unit evidence path.
    pub manifest: &'a str,
    /// Module-graph digest.
    pub graph_digest: &'a str,
    /// Normalized configured root.
    pub root: &'a str,
    /// Tool-input config digest.
    pub config_digest: &'a str,
    /// Root-lockfile slot, resolved against the checkout.
    pub lock_digest: DigestSlot,
}

/// Derive one proposal's identity extension before selection and reuse.
///
/// Proposal-owned facts (kind, drivers, declared inputs) come from
/// `identity`; workspace digests come from `inputs`. Spellings parse
/// fail-closed (validated proposals always parse).
///
/// # Errors
///
/// Returns [`ContractError`] for kind/driver/runner spellings outside
/// the known tokens.
pub fn extension_for_proposal(
    task: &ProposedTask,
    inputs: &TofuGroupExtensionInputs<'_>,
) -> Result<TofuTaskIdentityExtension, ContractError> {
    let root = crate::normalized_root_for_proposal(task)?;
    if inputs.root != root
        || inputs.unit_id != task.identity.unit_id
        || inputs.manifest != task.identity.unit_path
        || inputs.profile != task.configuration
    {
        return Err(ContractError::identity(
            "tofu_root",
            "extension_root_mismatch",
        ));
    }
    let unit_id = component_id_for_unit(inputs.unit_id, inputs.manifest);
    let identity = &task.identity;
    check_driver(&identity.compile_driver)?;
    check_runner(&identity.test_runner)?;
    let derived = ExtensionInputs {
        unit_id: &unit_id,
        workspace_id: inputs.workspace_id,
        profile: inputs.profile,
        manifest: inputs.manifest,
        graph_digest: inputs.graph_digest,
        root: inputs.root,
        config_digest: inputs.config_digest,
        lock_digest: inputs.lock_digest.clone(),
        kind: TofuTaskKind::parse(&task.task_kind)?,
        undeclared_reads: identity.undeclared_reads,
        declared_inputs: &identity.declared_inputs,
    };
    Ok(TofuTaskIdentityExtension::for_task(&derived))
}

/// Full adapter entry metadata for one neutral tofu proposal.
///
/// Driver/runner spellings check fail-closed (validated proposals
/// always carry the tofu spellings).
///
/// # Errors
///
/// Returns [`ContractError`] for driver/runner spellings outside the
/// known tokens.
pub fn entry_metadata_for_task(
    task: &ProposedTask,
    evidence_ids: &[String],
) -> Result<serde_json::Value, ContractError> {
    crate::normalized_root_for_proposal(task)?;
    check_driver(&task.identity.compile_driver)?;
    check_runner(&task.identity.test_runner)?;
    Ok(serde_json::json!({
        "unit_id": task.identity.unit_id,
        "display_name": task.display_name,
        "manifest_key": task.identity.unit_key,
        "kind": task.task_kind,
        "configuration": task.configuration,
        "target": task.identity.target,
        "compile_driver": task.identity.compile_driver,
        "test_runner": task.identity.test_runner,
        "evidence_ids": evidence_ids,
    }))
}

/// Root-lockfile slot at `root`: content, proven absence, or unknown.
///
/// `unit_path` is the unit evidence path (`.` or the root directory).
/// Delegates to the canonical root-lock digest function.
#[must_use]
pub fn lock_slot_at_root(root: &Path, unit_path: &str, reads: &mut FileCache) -> DigestSlot {
    lock_digest_at_root(root, unit_path, reads)
}

/// Fail-closed check for the tofu compile-driver spelling.
fn check_driver(value: &str) -> Result<(), ContractError> {
    if value == TOFU_DRIVER {
        Ok(())
    } else {
        Err(ContractError::identity(
            "compile_driver",
            format!("unknown_driver:{value}"),
        ))
    }
}

/// Fail-closed check for the tofu test-runner spelling.
fn check_runner(value: &str) -> Result<(), ContractError> {
    if value == TOFU_RUNNER {
        Ok(())
    } else {
        Err(ContractError::identity(
            "test_runner",
            format!("unknown_runner:{value}"),
        ))
    }
}
