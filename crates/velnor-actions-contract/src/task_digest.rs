//! Canonical obligation execution digest shared by generators and compilers.

use crate::{
    ContractError, HelperObligationDescriptor, NativeValidationDescriptor, canonical_json_bytes,
    digest_b3,
};
use serde::Serialize;

/// Bind the fixed command, toolchain, and optional compiled native authority.
///
/// Absent helper and native fields are omitted from the canonical preimage.
/// # Errors
/// Returns an error if canonical serialization fails.
pub fn canonical_task_digest(
    task_id: &str,
    argv: &[String],
    toolchain_id: &str,
    helper_obligation: Option<&HelperObligationDescriptor>,
    native_recipe: Option<&NativeValidationDescriptor>,
) -> Result<String, ContractError> {
    Ok(digest_b3(&canonical_json_bytes(&TaskDigestInputs {
        native_recipe,
        helper_obligation,
        task_id,
        argv,
        toolchain_id,
    })?))
}

/// Canonical preimage; field omissions preserve existing obligation identities.
#[derive(Debug, Serialize)]
struct TaskDigestInputs<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    native_recipe: Option<&'a NativeValidationDescriptor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    helper_obligation: Option<&'a HelperObligationDescriptor>,
    task_id: &'a str,
    argv: &'a [String],
    toolchain_id: &'a str,
}

#[cfg(test)]
#[path = "task_digest_tests.rs"]
mod tests;
