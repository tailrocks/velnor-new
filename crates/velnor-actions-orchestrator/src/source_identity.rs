//! Shared proposal identities for planning and source-qualified verification.
//!
//! This module hashes adapter proposals; callers establish source authority.
//! No scheduling or reuse decision is made here.

use crate::internal::internal_contract;
use crate::internal_plan::closure::resolve_closure_at_root;
use crate::internal_plan::identities::{
    ExtensionBundle, execution_identity_for, extension_bundle_with_snapshot, platform_id_for_group,
};
use crate::internal_plan::snapshot::{ExecutionSnapshot, canonical_digest};
use crate::internal_plan::{
    IdentityInputs, nextest_config_for, task_identity_digest, toolchain_id_for_runner,
};
use crate::{OrchestratorError, discover::Discovery};
use std::path::Path;
use velnor_actions_contract::{PlanGenerator, ProposedTask, Stack, StackExtension};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::extension_for_proposal;

/// Generation dimensions needed to reconstruct one proposal's identities.
pub(crate) struct SourceIdentityInputs<'a> {
    pub(crate) discovery: &'a Discovery,
    pub(crate) task: &'a ProposedTask,
    pub(crate) root: &'a Path,
    pub(crate) snapshot: &'a ExecutionSnapshot,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) generator: &'a PlanGenerator,
    pub(crate) label: &'a str,
}

/// Complete identity output from the existing adapter and closure chain.
pub(crate) struct ResolvedSourceIdentity {
    pub(crate) helper_record: Option<velnor_actions_contract::CompiledSourceHelper>,
    pub(crate) argv: Vec<String>,
    pub(crate) toolchain_id: String,
    pub(crate) platform_id: String,
    pub(crate) task_digest: String,
    pub(crate) execution_identity: velnor_actions_contract::TaskExecutionIdentity,
    pub(crate) helper_obligation: Option<velnor_actions_contract::HelperObligationDescriptor>,
    pub(crate) native_recipe: Option<velnor_actions_contract::NativeValidationDescriptor>,
    pub(crate) bundle: ExtensionBundle,
    pub(crate) closure_digest: String,
    pub(crate) input_digest: String,
}

/// Rebuild identities without planning execution or trusting serialized recipes.
pub(crate) fn resolve(
    inputs: &SourceIdentityInputs<'_>,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<ResolvedSourceIdentity, OrchestratorError> {
    let runner_label = crate::workloads::runner_for_task(inputs.task, inputs.label);
    let toolchain = toolchain_id_for_runner(inputs.task, inputs.catalog, runner_label)
        .map_err(internal_contract)?;
    let argv = crate::vectors::task_argv_for_runner(inputs.task, inputs.catalog, runner_label)?;
    let platform = platform_id_for_group(inputs.label, inputs.task).map_err(internal_contract)?;
    resolved_identity(inputs, &argv, &toolchain, &platform, reads)
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
    if Stack::from_id(&task.stack_id) == Some(Stack::Workload) {
        return Ok((
            crate::internal_plan::workload_identity::extension_for(task, bundle)
                .map_err(internal_contract)?,
            false,
        ));
    }
    if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
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
fn resolved_identity(
    inputs: &SourceIdentityInputs<'_>,
    argv: &[String],
    toolchain: &str,
    platform_id: &str,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<ResolvedSourceIdentity, OrchestratorError> {
    let task = inputs.task;
    let binding = crate::helper_obligation_binding::binding_for_proposal(
        task,
        inputs.catalog,
        &inputs.generator.version,
        inputs.label,
    )?;
    let (native_recipe, helper_obligation, helper_record) =
        binding.map_or((None, None, None), |binding| {
            (
                binding.native_recipe,
                Some(binding.descriptor),
                Some(binding.record),
            )
        });
    let nextest_config = nextest_config_for(inputs.discovery, task);
    let bundle = extension_bundle_with_snapshot(
        inputs.snapshot,
        inputs.discovery,
        task,
        Some(inputs.root),
        nextest_config.as_deref(),
    );
    let (extension, _) = extension_for_task(task, inputs.root, &bundle, reads)?;
    let closure = resolve_closure_at_root(
        inputs.root,
        task,
        nextest_config.as_deref(),
        bundle.graph_digest(),
        toolchain,
        platform_id,
        &mut *reads,
        Some(inputs.snapshot.checkout_inputs()),
    )
    .map_err(internal_contract)?;
    let closure_digest = canonical_digest(&closure).map_err(internal_contract)?;
    let input_digest = task_identity_digest(&IdentityInputs {
        task,
        argv,
        toolchain_id: toolchain,
        platform_id,
        manifest: &task.identity.unit_path,
        generator: inputs.generator,
        extension,
        closure_digest: &closure_digest,
        helper_obligation: helper_obligation.as_ref(),
        native_recipe: native_recipe.as_ref(),
    })
    .map_err(internal_contract)?;
    let execution_identity =
        execution_identity_for(task, &bundle, inputs.catalog, toolchain, platform_id)
            .map_err(internal_contract)?;
    Ok(ResolvedSourceIdentity {
        helper_record,
        argv: argv.to_vec(),
        toolchain_id: toolchain.to_owned(),
        platform_id: platform_id.to_owned(),
        task_digest: super::task_digest(
            &task.task_id,
            argv,
            toolchain,
            helper_obligation.as_ref(),
            native_recipe.as_ref(),
        )
        .map_err(internal_contract)?,
        execution_identity,
        native_recipe,
        helper_obligation,
        bundle,
        closure_digest,
        input_digest,
    })
}
