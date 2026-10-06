//! Complete native program reconstruction; SDK launch authority stays neutral.
use crate::{
    SupportBundle,
    homebrew::{audit, package_update, preparation},
};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, ContractError, HelperInvocation, NativeValidationDescriptor,
    SourceBoundHelper, SourceBoundOperation, compiled_source_sha256, generated_source,
    workflow::native_tools::CompiledNativeExecRecipe,
};

/// Rebuild fixed semantic arguments and complete native-owned source bytes.
/// SDK recipe authority must be checked by orchestration before composition.
/// # Errors
/// Rejects invalid semantics, absent reviewed pins or mismatched launch domain.
pub fn record_for_descriptor(
    descriptor: &NativeValidationDescriptor,
    reviewed: Option<&audit::BrewSourceIdentity>,
    recipe: CompiledNativeExecRecipe,
    version: &str,
) -> Result<CompiledSourceHelper, ContractError> {
    descriptor.validate()?;
    recipe.validate()?;
    if recipe
        .environment()
        .get("VELNOR_NATIVE_VALIDATION_SOURCE_SHA")
        .map(String::as_str)
        != Some("${{ github.sha }}")
    {
        return Err(invalid("missing_closed_source_context"));
    }
    let (operation, args, bundle, identity) = match descriptor {
        NativeValidationDescriptor::PackageUpdateFixture { profile } => {
            if recipe.is_homebrew_foundation()
                || recipe.installed_selectors().len() != tools(descriptor).len()
            {
                return Err(invalid("package_update_tool_domain"));
            }
            (
                package_update::OPERATION,
                package_update::arguments(profile)?,
                package_update::support(version)?,
                "package-update-fixture-v1".to_owned(),
            )
        }
        NativeValidationDescriptor::HomebrewPreparation {
            repository,
            has_casks,
        } => {
            if !recipe.is_homebrew_foundation() {
                return Err(invalid("homebrew_tool_domain"));
            }
            let reviewed = reviewed.ok_or_else(|| invalid("homebrew_reviewed_source_missing"))?;
            let tap = audit::TapIdentity::from_repository(repository)?;
            let args = preparation::preparation_arguments(reviewed, &tap, *has_casks);
            let bundle = preparation::preparation_for_arguments(reviewed, &args, version)?;
            (
                SourceBoundOperation::HomebrewPreparation,
                args,
                bundle,
                reviewed.recipe(),
            )
        }
    };
    let identity = velnor_actions_contract::digest_b3(identity.as_bytes());
    if recipe.environment().get("VELNOR_NATIVE_VALIDATION_RECIPE") != Some(&identity) {
        return Err(invalid("source_recipe_identity_mismatch"));
    }
    source_record(descriptor, operation, args, &bundle, recipe, version)
}

/// Semantic requirements; numeric selectors remain exclusively SDK-owned.
#[must_use]
pub fn tools(descriptor: &NativeValidationDescriptor) -> Vec<package_update::PackageUpdateTool> {
    match descriptor {
        NativeValidationDescriptor::PackageUpdateFixture { .. } => package_update::tools().to_vec(),
        NativeValidationDescriptor::HomebrewPreparation { .. } => Vec::new(),
    }
}

/// Exact native reconstruction after orchestration verifies the SDK recipe.
/// # Errors
/// Rejects any changed semantic metadata, source binding, arguments or environment.
pub fn record_for_invocation(
    invocation: &HelperInvocation,
    environment: &BTreeMap<String, String>,
    reviewed: Option<&audit::BrewSourceIdentity>,
    recipe: CompiledNativeExecRecipe,
    version: &str,
) -> Result<CompiledSourceHelper, ContractError> {
    let semantic = invocation
        .native_validation_descriptor()
        .ok_or_else(|| invalid("native_validation_descriptor_missing"))?;
    let expected = record_for_descriptor(semantic, reviewed, recipe, version)?;
    if expected.invocation() != invocation || expected.environment() != environment {
        return Err(invalid("native_validation_authority_mismatch"));
    }
    Ok(expected)
}

fn source_record(
    semantic: &NativeValidationDescriptor,
    operation: SourceBoundOperation,
    args: Vec<String>,
    bundle: &SupportBundle,
    recipe: CompiledNativeExecRecipe,
    version: &str,
) -> Result<CompiledSourceHelper, ContractError> {
    let [file] = bundle.files() else {
        return Err(invalid("incomplete_source_closure"));
    };
    if file.path() != operation.path() {
        return Err(invalid("source_path"));
    }
    let body = format!("{}\n{}", SOURCE_CHECK, file.source());
    let source = generated_source(version, &body)?;
    let binding = SourceBoundHelper::compiled(
        operation,
        file.path(),
        &compiled_source_sha256(source.as_bytes()),
    )?;
    let invocation =
        HelperInvocation::compiled(binding, args, recipe.installed_selectors().to_vec())?
            .with_native_validation_descriptor(semantic.clone())?;
    CompiledSourceHelper::compiled(invocation, source)?.with_execution_recipe(recipe)
}

const SOURCE_CHECK: &str = "set -eu\ntest -n \"${VELNOR_NATIVE_VALIDATION_SOURCE_SHA:-}\"\nactual_head=$(/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin HOME=\"$HOME\" GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null git -C . rev-parse --verify HEAD)\ntest \"$actual_head\" = \"$VELNOR_NATIVE_VALIDATION_SOURCE_SHA\" || { echo native_validation_source_mismatch >&2; exit 1; }";

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("native_validation", reason)
}

#[cfg(test)]
#[path = "validation_tests.rs"]
mod tests;
