//! Task identity and input closure for planning and source evidence.

use std::path::Path;

use velnor_actions_contract::{PlanGenerator, ProposedTask, Stack, StackExtension};
use velnor_actions_rust::extension_for_proposal;

use crate::OrchestratorError;
use crate::discover::Discovery;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::{ExtensionBundle, extension_bundle_with_snapshot};
use crate::internal_plan::snapshot::{ExecutionSnapshot, canonical_digest};
use crate::internal_plan::{IdentityInputs, nextest_config_for, task_identity_digest};

use super::GroupInputs;

pub(super) type PlannedIdentity = TaskIdentityDerivation;

/// Extension bundle with input digests for one task.
pub(crate) struct TaskIdentityDerivation {
    /// Bundle with lockfile and Nextest digests for source files in this checkout.
    pub(crate) bundle: ExtensionBundle,
    /// Whether the stack extension permits reuse.
    pub(crate) reuse_eligible: bool,
    /// Digest over the complete input closure in the JSON format given by the contract.
    pub(crate) closure_digest: String,
    /// Input digest with the closure digest bound into the envelope.
    pub(crate) input_digest: String,
}

/// Inputs used to make task identities before and after a task.
pub(crate) struct TaskIdentityDerivationInputs<'a> {
    /// Repository checkout.
    pub(crate) root: &'a Path,
    /// Current source inventory.
    pub(crate) discovery: &'a Discovery,
    /// Universe member to identify.
    pub(crate) task: &'a ProposedTask,
    /// Fixed command vector.
    pub(crate) argv: &'a [String],
    /// Selected toolchain digest.
    pub(crate) toolchain: &'a str,
    /// Selected platform digest.
    pub(crate) platform_id: &'a str,
    /// Generator identity recorded by the plan.
    pub(crate) generator: &'a PlanGenerator,
    /// Snapshot of the same source inventory.
    pub(crate) snapshot: &'a ExecutionSnapshot,
}

/// Make the extension and input closure for one task from this source tree.
///
/// Rust and `ToFu` adapters read the source and lockfile inputs.
/// Both use the same identity format as the plan.
/// # Errors
pub(crate) fn derive_task_identity(
    inputs: &TaskIdentityDerivationInputs<'_>,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<TaskIdentityDerivation, OrchestratorError> {
    let task = inputs.task;
    let nextest_config = nextest_config_for(inputs.discovery, task);
    let bundle = extension_bundle_with_snapshot(
        inputs.snapshot,
        inputs.discovery,
        task,
        Some(inputs.root),
        nextest_config.as_deref(),
    );
    let (extension, reuse_eligible) = extension_for_task(task, inputs.root, &bundle, reads)?;
    let closure = if Stack::from_id(&task.stack_id) == Some(Stack::Rust) {
        inputs
            .snapshot
            .resolve_rust_closure(
                inputs.root,
                task,
                nextest_config.as_deref(),
                bundle.graph_digest(),
                inputs.toolchain,
                inputs.platform_id,
            )
            .map_err(internal_contract)?
    } else {
        resolve_closure_at_root(
            inputs.root,
            task,
            nextest_config.as_deref(),
            bundle.graph_digest(),
            inputs.toolchain,
            inputs.platform_id,
            reads,
        )
        .map_err(internal_contract)?
    };
    let closure_digest = canonical_digest(&closure).map_err(internal_contract)?;
    let input_digest = task_identity_digest(&IdentityInputs {
        task,
        argv: inputs.argv,
        toolchain_id: inputs.toolchain,
        platform_id: inputs.platform_id,
        manifest: &task.identity.unit_path,
        generator: inputs.generator,
        extension,
        closure_digest: &closure_digest,
    })
    .map_err(internal_contract)?;
    Ok(TaskIdentityDerivation {
        bundle,
        reuse_eligible,
        closure_digest,
        input_digest,
    })
}

/// Preserve the plan-group entry point while sharing the public derivation contract.
pub(super) fn planned_identity(
    inputs: &GroupInputs<'_>,
    argv: &[String],
    toolchain: &str,
    platform_id: &str,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<PlannedIdentity, OrchestratorError> {
    derive_task_identity(
        &TaskIdentityDerivationInputs {
            root: inputs.root,
            discovery: inputs.discovery,
            task: inputs.task,
            argv,
            toolchain,
            platform_id,
            generator: inputs.wire.generator,
            snapshot: inputs.snapshot,
        },
        reads,
    )
}

/// Stack-extension envelope plus reuse eligibility for one task.
pub(super) fn extension_for_task(
    task: &ProposedTask,
    root: &Path,
    bundle: &ExtensionBundle,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<(StackExtension, bool), OrchestratorError> {
    let stack = Stack::require_known(&task.stack_id).map_err(internal_contract)?;
    if stack == Stack::Mise {
        return Err(internal("named_check_definition_required"));
    }
    if stack == Stack::Tofu {
        let ext = crate::internal_plan::tofu_extension_for(task, root, bundle, reads)
            .map_err(internal_contract)?;
        return Ok((ext.to_stack_extension(), ext.reuse_eligible().is_ok()));
    }
    let ext = extension_for_proposal(task, &bundle.inputs()).map_err(internal_contract)?;
    Ok((ext.to_stack_extension(), ext.reuse_eligible().is_ok()))
}
