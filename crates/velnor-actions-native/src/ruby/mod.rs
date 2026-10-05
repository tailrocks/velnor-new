//! Ruby syntax obligations. Source paths are data, never executable fragments.

use velnor_actions_contract::ContractError;

/// Propose Ruby's fixed compilation-only source validation command.
/// # Errors
/// Rejects empty, option-like, absolute or traversing source paths.
pub fn syntax(paths: &[String]) -> Result<Vec<String>, ContractError> {
    if paths.is_empty() {
        return Err(ContractError::identity("ruby", "empty_source_inventory"));
    }
    for path in paths {
        if path.starts_with('-')
            || velnor_actions_contract::normalize_posix_path(path)? != *path
            || path.chars().any(char::is_control)
        {
            return Err(ContractError::identity("ruby", "invalid_source_path"));
        }
    }
    let mut argv = vec![
        "ruby".to_owned(),
        "-e".to_owned(),
        "ARGV.each { |path| RubyVM::InstructionSequence.compile_file(path) }".to_owned(),
        "--".to_owned(),
    ];
    argv.extend_from_slice(paths);
    Ok(argv)
}
