//! Fixed command validation, with exact renderer-owned script exceptions.

use std::collections::BTreeMap;

use crate::{RenderError, steps::scan_for_private_subcommands};

/// Validate an untrusted fixed argument vector without shell fragments.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] or [`RenderError::PrivateSubcommand`].
pub fn validate_command_argv(argv: &[String]) -> Result<(), RenderError> {
    validate_argv(argv, None)
}

/// Validate one named step. Only exact renderer-owned MBX scripts may contain LF.
///
/// The script is identified by its fixed step name, `bash -c` argv position,
/// and byte-for-byte body. The exact `env -u` prefix is skipped before that
/// shape is checked. Repository-supplied argv has no route into this exception.
///
/// # Errors
pub(crate) fn validate_step_command_argv(name: &str, argv: &[String]) -> Result<(), RenderError> {
    let script_index = crate::mbx_bundle::trusted_script_argument(name, argv);
    validate_argv(argv, script_index)
}

fn validate_argv(argv: &[String], script_index: Option<usize>) -> Result<(), RenderError> {
    if argv.is_empty() {
        return Err(RenderError::BadCommand("empty_argv".to_owned()));
    }
    let mut previous: Option<&str> = None;
    for (index, arg) in argv.iter().enumerate() {
        if arg.is_empty() {
            return Err(RenderError::BadCommand("empty_arg".to_owned()));
        }
        if arg
            .chars()
            .any(|ch| ch == '\0' || ch == '\r' || (ch == '\n' && script_index != Some(index)))
        {
            return Err(RenderError::BadCommand(format!("control_char:{arg}")));
        }
        if script_index != Some(index) && (arg.contains("$(") || arg.contains('`')) {
            return Err(RenderError::BadCommand(format!(
                "command_substitution:{arg}"
            )));
        }
        if has_background_op(arg) {
            return Err(RenderError::BadCommand(format!("background_shell:{arg}")));
        }
        if previous == Some("cargo") && arg == "install" {
            return Err(RenderError::BadCommand("cargo_install".to_owned()));
        }
        if is_absolute_cargo(arg) {
            return Err(RenderError::BadCommand(format!(
                "absolute_cargo_path:{arg}"
            )));
        }
        scan_for_private_subcommands(arg)?;
        previous = Some(arg);
    }
    Ok(())
}

/// Validate a fixed env map: `A-Z0-9_` keys, single-line clean values.
///
/// # Errors
pub fn validate_env(env: &BTreeMap<String, String>) -> Result<(), RenderError> {
    for (key, value) in env {
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(RenderError::BadCommand(format!("bad_env_key:{key}")));
        }
        if value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
        {
            return Err(RenderError::BadCommand(format!("bad_env_value:{key}")));
        }
        crate::expressions::check_env_value(key, value)?;
        scan_for_private_subcommands(key)?;
        scan_for_private_subcommands(value)?;
    }
    Ok(())
}

/// True for `/cargo` or paths ending in `/cargo`.
fn is_absolute_cargo(arg: &str) -> bool {
    arg.starts_with('/') && (arg == "/cargo" || arg.ends_with("/cargo"))
}

/// True for shell background operators (`&`), never for `&&`/redirections.
fn has_background_op(arg: &str) -> bool {
    if arg.contains(" & ") {
        return true;
    }
    let trimmed = arg.trim();
    if let Some(head) = trimmed.strip_prefix('&') {
        return !head.starts_with('&');
    }
    if let Some(body) = trimmed.strip_suffix('&') {
        return !body.ends_with('&');
    }
    false
}
