//! Closed source-owner resolution for persisted native obligations.
//!
//! Wire helper invocations never grant execution authority. An obligation's
//! stack, phase and configuration select its compiled owner; absent qualified
//! owner factories remain hard errors, including when metadata is removed.

use velnor_actions_contract::{
    CompiledSourceHelper, HelperObligationDescriptor, MatrixEntry, NativeValidationDescriptor,
    Plan, ProposedTask, Stack,
};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::internal::internal;

#[path = "helper_obligation_verification.rs"]
mod verification;

/// Whether the task identity requires a compiled native helper obligation.
pub(crate) fn requires_helper_obligation(entry: &MatrixEntry) -> bool {
    requires_helper_task(&entry.task_id)
}

/// Deny missing compiled bindings for closed helper tasks; grants no authority.
pub(crate) fn requires_helper_task(task_id: &str) -> bool {
    crate::extension_schemas::task_stack_segment(task_id)
        .is_some_and(|stack| owner_for_task(stack, task_id).is_some())
}

/// Independently owned semantics plus their exact compiled execution identity.
pub(crate) struct ProposalHelperBinding {
    pub(crate) native_recipe: Option<NativeValidationDescriptor>,
    pub(crate) record: CompiledSourceHelper,
    pub(crate) descriptor: HelperObligationDescriptor,
}

/// Build semantics, compiled record and descriptor once before task hashing.
pub(crate) fn binding_for_proposal(
    task: &ProposedTask,
    catalog: &ToolCatalog,
    version: &str,
    label: &str,
) -> Result<Option<ProposalHelperBinding>, OrchestratorError> {
    let native_recipe = crate::workloads::native_descriptor::from_proposal(task)?;
    let Some(owner) = owner_for_task(&task.stack_id, &task.task_id) else {
        return Ok(None);
    };
    if version != env!("CARGO_PKG_VERSION") {
        return Err(internal("helper_generator_version_mismatch"));
    }
    if crate::extension_schemas::task_kind_segment(&task.task_id) != Some(task.task_kind.as_str())
        || task.task_id.rsplit('/').next() != Some(task.configuration.as_str())
    {
        return Err(internal("helper_proposal_identity_mismatch"));
    }
    let record = match owner {
        HelperOwner::HomebrewPreparation | HelperOwner::PackageUpdateFixture => {
            let semantic = native_recipe
                .as_ref()
                .ok_or_else(|| internal("helper_native_semantic_descriptor_missing"))?;
            native_record(semantic, &owner, catalog, version)?
        }
        HelperOwner::TofuCachedInit => {
            let actual_label = crate::workloads::runner_for_task(task, label);
            let host = crate::workloads::host_for_runner(actual_label)?;
            crate::tofu_cached_init::from_proposal(task, catalog, host, version)?
        }
        HelperOwner::DesktopNative => return Err(internal(owner.pending_reason())),
    };
    let id = velnor_actions_contract::matrix_id_for_task_group(&task.stack_id, &task.task_id)?;
    let key = velnor_actions_contract::matrix_key_for_id(&id)?;
    let descriptor = HelperObligationDescriptor::from_compiled(&record, &key)?;
    Ok(Some(ProposalHelperBinding {
        native_recipe,
        record,
        descriptor,
    }))
}

/// Reconstruct execution authority from its compiled source owner.
///
/// Generic serialized invocations, source hashes and environment maps never
/// authorize execution. The selected closed owner reconstructs the complete
/// record using its semantic recipe and the generator's pinned tool catalog.
pub(crate) fn record_for_entry(
    plan: &Plan,
    entry: &MatrixEntry,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    entry.validate(&plan.run_key)?;
    if plan.generator.version != env!("CARGO_PKG_VERSION") {
        return Err(internal("helper_generator_version_mismatch"));
    }
    if crate::extension_schemas::task_stack_segment(&entry.task_id) != Some(entry.stack_id.as_str())
    {
        return Err(internal("helper_owner_stack_mismatch"));
    }
    let owner = owner_for_task(&entry.stack_id, &entry.task_id)
        .ok_or_else(|| internal("helper_obligation_owner_unknown"))?;
    match owner {
        HelperOwner::DesktopNative => Err(internal(owner.pending_reason())),
        _ => verification::record_for_entry(plan, entry),
    }
}

/// Lower semantic data only through the selected compiled SDK owner.
fn native_record(
    semantic: &NativeValidationDescriptor,
    owner: &HelperOwner,
    catalog: &ToolCatalog,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    if !matches!(
        (owner, semantic),
        (
            HelperOwner::HomebrewPreparation,
            NativeValidationDescriptor::HomebrewPreparation { .. }
        ) | (
            HelperOwner::PackageUpdateFixture,
            NativeValidationDescriptor::PackageUpdateFixture { .. }
        )
    ) {
        return Err(internal("helper_native_owner_mismatch"));
    }
    let recipe =
        velnor_actions_mise::catalog::native_validation::recipe_for_descriptor(semantic, catalog)
            .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    velnor_actions_mise::catalog::native_validation::validate_recipe(semantic, catalog, &recipe)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    let tools = velnor_actions_native::homebrew::validation::tools(semantic)
        .into_iter()
        .map(|tool| match tool {
            velnor_actions_native::homebrew::package_update::PackageUpdateTool::Ruby => {
                velnor_actions_mise::PinnedTool::Ruby
            }
            velnor_actions_native::homebrew::package_update::PackageUpdateTool::Jq => {
                velnor_actions_mise::PinnedTool::Jq
            }
        })
        .collect::<Vec<_>>();
    let selectors = catalog
        .tool_specs(&tools)
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    if recipe.installed_selectors() != selectors {
        return Err(internal("helper_native_tool_requirements_mismatch"));
    }
    let reviewed = matches!(owner, HelperOwner::HomebrewPreparation)
        .then(reviewed_homebrew)
        .transpose()?;
    velnor_actions_native::homebrew::validation::record_for_descriptor(
        semantic,
        reviewed.as_ref(),
        recipe,
        version,
    )
    .map_err(OrchestratorError::from)
}

fn reviewed_homebrew()
-> Result<velnor_actions_native::homebrew::audit::BrewSourceIdentity, OrchestratorError> {
    use velnor_actions_mise::catalog::homebrew;
    Ok(
        velnor_actions_native::homebrew::audit::BrewSourceIdentity::reviewed(
            homebrew::VERSION,
            homebrew::SOURCE_SHA,
            homebrew::PORTABLE_RUBY_VERSION,
            homebrew::PORTABLE_RUBY_X86_64_LINUX_SHA256,
            homebrew::PORTABLE_RUBY_ARM64_LINUX_SHA256,
        )?,
    )
}

/// Source factories are selected by immutable task identity, never metadata.
fn owner_for_task(stack: &str, task: &str) -> Option<HelperOwner> {
    if crate::extension_schemas::task_stack_segment(task) != Some(stack) {
        return None;
    }
    let phase = crate::extension_schemas::task_kind_segment(task)?;
    let base = velnor_actions_contract::split_shard_suffix(task).map_or(task, |(base, _, _)| base);
    let configuration = base.rsplit('/').next()?;
    match (Stack::from_id(stack)?, configuration, phase) {
        (Stack::Workload, "homebrew_audit", "homebrew-tap-local") => {
            Some(HelperOwner::HomebrewPreparation)
        }
        (Stack::Workload, "package_update_fixture", "package-update-fixtures") => {
            Some(HelperOwner::PackageUpdateFixture)
        }
        (
            Stack::Workload,
            "native_xcode_project_ci",
            "native-ffi" | "native-generate" | "native-xcode-build" | "native-xcode-test",
        )
        | (
            Stack::Workload,
            "native_swift_package_ci",
            "native-ffi" | "native-swift-build" | "native-swift-test",
        ) => Some(HelperOwner::DesktopNative),
        (Stack::Tofu, "default", "init") => Some(HelperOwner::TofuCachedInit),
        _ => None,
    }
}

#[cfg(test)]
#[path = "helper_obligation_binding_tests.rs"]
mod tests;

/// Closed owners with explicitly unqualified consumer boundaries.
enum HelperOwner {
    DesktopNative,
    HomebrewPreparation,
    PackageUpdateFixture,
    TofuCachedInit,
}

impl HelperOwner {
    const fn pending_reason(&self) -> &'static str {
        match self {
            Self::DesktopNative => "desktop_source_bound_primitive_qualification_pending",
            Self::HomebrewPreparation => "homebrew_exact_tool_authority_pending",
            Self::PackageUpdateFixture => "package_update_source_bound_fixture_pending",
            Self::TofuCachedInit => "tofu_cached_init_source_owner_qualification_pending",
        }
    }
}
