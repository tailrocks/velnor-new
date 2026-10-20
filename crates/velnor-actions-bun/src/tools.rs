//! Agreement between `packageManager` and the Mise-declared Bun.

use crate::BunError;

/// Check the root `packageManager` against the exact Bun version Mise declares.
///
/// An absent field is accepted. Anything other than `bun@<exact>[+hash]` is a
/// second package manager and fails.
///
/// # Errors
///
/// Returns `unsupported_package_manager` for a non-Bun manager and
/// `conflicting_project_tool_versions:bun` when the versions differ.
pub fn check_package_manager(
    package_manager: Option<&str>,
    mise_bun: &str,
) -> Result<(), BunError> {
    let Some(declared) = package_manager else {
        return Ok(());
    };
    let Some(rest) = declared.strip_prefix("bun@") else {
        return Err(BunError::new("unsupported_package_manager", declared));
    };
    let version = rest.split_once('+').map_or(rest, |(version, _)| version);
    if version == mise_bun {
        Ok(())
    } else {
        Err(BunError::new("conflicting_project_tool_versions", "bun"))
    }
}
