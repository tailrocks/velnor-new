//! Shell source validation obligations.

use velnor_actions_contract::ContractError;

/// Propose Shellcheck validation for explicitly selected source paths.
/// # Errors
/// Rejects empty, option-like, absolute or traversing source paths.
pub fn check(paths: &[String]) -> Result<Vec<String>, ContractError> {
    if paths.is_empty() {
        return Err(ContractError::identity("shell", "empty_source_inventory"));
    }
    for path in paths {
        if path.starts_with('-')
            || velnor_actions_contract::normalize_posix_path(path)? != *path
            || path.chars().any(char::is_control)
        {
            return Err(ContractError::identity("shell", "invalid_source_path"));
        }
    }
    let mut argv = vec!["shellcheck".to_owned()];
    argv.extend_from_slice(paths);
    Ok(argv)
}
