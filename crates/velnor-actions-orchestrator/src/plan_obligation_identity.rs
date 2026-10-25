use std::path::Path;

use velnor_actions_contract::{ProposedTask, Stack, StackExtension};
use velnor_actions_rust::extension_for_proposal;

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::{ExtensionBundle, extension_bundle_with_snapshot};
use crate::internal_plan::snapshot::canonical_digest;
use crate::internal_plan::{IdentityInputs, nextest_config_for, task_identity_digest};

use super::GroupInputs;

/// Extension bundle plus closure-bound identity digests for one task.
pub(super) struct PlannedIdentity {
    /// Snapshot-indexed bundle with checkout-bound lock/Nextest digests.
    pub(super) bundle: ExtensionBundle,
    /// Whether the stack extension permits reuse.
    pub(super) reuse_eligible: bool,
    /// Canonical digest over the complete input closure.
    pub(super) closure_digest: String,
    /// Input digest with the closure digest bound into the envelope.
    pub(super) input_digest: String,
}

/// Stack-extension envelope plus reuse eligibility for one task.
///
/// Closed per-stack dispatch: rust tasks derive through the rust
/// bridge over the snapshot bundle; tofu tasks derive through the
/// tofu bridge with a checkout-bound root-lockfile slot. Both feed
/// the same neutral envelope and gate.
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

/// Snapshot bundle plus closure-bound identity digests for one task.
///
/// The closure resolves against the checkout and its digest binds into
/// the identity envelope, so a source edit flips `input_digest` even
/// when the changed-work hint misses it.
pub(super) fn planned_identity(
    inputs: &GroupInputs<'_>,
    argv: &[String],
    toolchain: &str,
    platform_id: &str,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<PlannedIdentity, OrchestratorError> {
    let task = inputs.task;
    let manifest = task.identity.unit_path.clone();
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
                toolchain,
                platform_id,
            )
            .map_err(internal_contract)?
    } else {
        resolve_closure_at_root(
            inputs.root,
            task,
            nextest_config.as_deref(),
            bundle.graph_digest(),
            toolchain,
            platform_id,
            &mut *reads,
        )
        .map_err(internal_contract)?
    };
    let closure_digest = canonical_digest(&closure).map_err(internal_contract)?;
    let input_digest = task_identity_digest(&IdentityInputs {
        task,
        argv,
        toolchain_id: toolchain,
        platform_id,
        manifest: &manifest,
        generator: inputs.wire.generator,
        extension,
        closure_digest: &closure_digest,
    })
    .map_err(internal_contract)?;
    Ok(PlannedIdentity {
        bundle,
        reuse_eligible,
        closure_digest,
        input_digest,
    })
}
