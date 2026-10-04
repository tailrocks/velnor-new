//! Closed runner output capability for immutable owner-qualified producers.
use super::{CompiledSourceHelper, HelperInvocation, SourceBoundOperation};
use crate::ContractError;
use crate::workflow::native_tools::NativeCredentialScope;

pub(super) fn eligible(invocation: &HelperInvocation) -> bool {
    match invocation.descriptor().operation() {
        SourceBoundOperation::OciDelivery => true,
        SourceBoundOperation::RustReleaseSourceSnapshot
        | SourceBoundOperation::RustReleasePreparedPackage => invocation.args().is_empty(),
        _ => false,
    }
}

pub(super) fn validate(record: &CompiledSourceHelper) -> Result<(), ContractError> {
    if !record.github_output() {
        return Ok(());
    }
    if !eligible(record.invocation())
        || record.environment().contains_key("GITHUB_OUTPUT")
        || record.execution_recipe().is_none()
        || !scope_matches(record)
    {
        return Err(ContractError::identity(
            "source_helper",
            "output_capability",
        ));
    }
    Ok(())
}

fn scope_matches(record: &CompiledSourceHelper) -> bool {
    let expected = match record.invocation().descriptor().operation() {
        SourceBoundOperation::RustReleaseSourceSnapshot => NativeCredentialScope::GithubReadOnly,
        SourceBoundOperation::RustReleasePreparedPackage => NativeCredentialScope::Anonymous,
        _ => return true,
    };
    record
        .execution_recipe()
        .is_some_and(|recipe| recipe.credential_scope() == expected)
}

#[cfg(test)]
#[path = "source_helper_prepared_output_tests.rs"]
mod prepared_tests;
