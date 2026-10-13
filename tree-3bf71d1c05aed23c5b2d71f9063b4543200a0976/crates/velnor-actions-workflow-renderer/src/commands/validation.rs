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
    let shell_script_index = inline_shell_script_index(argv)?;
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
        // Only the exact named MBX script may contain substitutions.
        if script_index != Some(index) && (arg.contains("$(") || arg.contains('`')) {
            return Err(RenderError::BadCommand(format!(
                "command_substitution:{arg}"
            )));
        }
        if shell_script_index == Some(index) && has_background_op(arg) {
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

/// Return the script argument only when argv is an inline shell invocation.
///
/// Accept only bounded `env` wrappers and shell options. Reject wrappers or
/// options that hide an inline script from the validator.
pub(super) fn inline_shell_script_index(argv: &[String]) -> Result<Option<usize>, RenderError> {
    let mut command_index = 0;
    let mut env_depth = 0;
    loop {
        let Some(command) = argv.get(command_index) else {
            return Ok(None);
        };
        if is_supported_shell(command) {
            return shell_script_argument_index(argv, command_index);
        }
        if !is_executable_named(command, "env") {
            return Ok(None);
        }
        if env_depth == MAX_ENV_WRAPPER_DEPTH {
            return Err(unsupported_shell_wrapper());
        }
        env_depth += 1;
        let Some(next_command) = env_command_index(argv, command_index)? else {
            return Ok(None);
        };
        command_index = next_command;
    }
}

fn shell_script_argument_index(
    argv: &[String],
    shell_index: usize,
) -> Result<Option<usize>, RenderError> {
    let mut index = shell_index + 1;
    let mut unsupported_option = false;
    while let Some(option) = argv.get(index) {
        if option == "--" {
            return if unsupported_option {
                Err(unsupported_shell_options())
            } else {
                Ok(None)
            };
        }
        if option == "-c" {
            if unsupported_option {
                return Err(unsupported_shell_options());
            }
            return argv
                .get(index + 1)
                .map(|_| Some(index + 1))
                .ok_or_else(unsupported_shell_options);
        }
        if let Some(short_options) = option.strip_prefix('-') {
            if short_options.is_empty() {
                return Ok(None);
            }
            if short_options.starts_with('-') {
                unsupported_option = true;
                index += 1;
                if matches!(option.as_str(), "--rcfile" | "--init-file") {
                    index += 1;
                }
                continue;
            }
            if short_options.ends_with('c') {
                if short_options[..short_options.len() - 1]
                    .chars()
                    .all(is_supported_shell_flag)
                    && !unsupported_option
                {
                    return argv
                        .get(index + 1)
                        .map(|_| Some(index + 1))
                        .ok_or_else(unsupported_shell_options);
                }
                return Err(unsupported_shell_options());
            }
            if short_options.contains('c') {
                return Err(unsupported_shell_options());
            }
            if short_options.chars().all(is_supported_shell_flag) {
                index += 1;
                continue;
            }
            unsupported_option = true;
            index += 1;
            if matches!(option.as_str(), "-o" | "-O") {
                index += 1;
            }
            continue;
        }
        if option.starts_with('+') {
            unsupported_option = true;
            index += 1;
            if matches!(option.as_str(), "+o" | "+O") {
                index += 1;
            }
            continue;
        }
        if unsupported_option {
            return Err(unsupported_shell_options());
        }
        return Ok(None);
    }
    if unsupported_option {
        Err(unsupported_shell_options())
    } else {
        Ok(None)
    }
}

/// Bound env-to-env command wrappers before a shell.
const MAX_ENV_WRAPPER_DEPTH: usize = 4;

fn is_supported_shell(command: &str) -> bool {
    is_executable_named(command, "sh") || is_executable_named(command, "bash")
}

fn is_executable_named(command: &str, expected: &str) -> bool {
    command
        .rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .is_some_and(|basename| basename == expected)
}

const fn is_supported_shell_flag(flag: char) -> bool {
    matches!(flag, 'e' | 'u' | 'x')
}

fn unsupported_shell_options() -> RenderError {
    RenderError::BadCommand("unsupported_inline_shell_options".to_owned())
}

/// Find the command after the supported `env` prefixes.
fn env_command_index(argv: &[String], env_index: usize) -> Result<Option<usize>, RenderError> {
    let mut index = env_index + 1;
    loop {
        let Some(arg) = argv.get(index) else {
            return Ok(None);
        };
        match arg.as_str() {
            "--" => {
                index += 1;
                while argv
                    .get(index)
                    .is_some_and(|value| is_env_assignment(value))
                {
                    index += 1;
                }
                return Ok(Some(index));
            }
            "-u" | "--unset" => {
                if argv.get(index + 1).is_none() {
                    return Err(unsupported_shell_wrapper());
                }
                index += 2;
            }
            "-i" | "--ignore-environment" | "-" | "-0" | "--null" | "-v" | "--debug" => {
                index += 1;
            }
            "-C" | "--chdir" => {
                if argv.get(index + 1).is_none() {
                    return Err(unsupported_shell_wrapper());
                }
                index += 2;
            }
            "-S" | "--split-string" => return Err(unsupported_shell_wrapper()),
            value if value.starts_with("--unset=") => index += 1,
            value if value.starts_with("--chdir=") => index += 1,
            value if value.starts_with("--split-string=") => {
                return Err(unsupported_shell_wrapper());
            }
            "--help" | "--version" => return Ok(None),
            value if is_env_assignment(value) => index += 1,
            value if value.starts_with('-') => return Err(unsupported_shell_wrapper()),
            _ => return Ok(Some(index)),
        }
    }
}

fn unsupported_shell_wrapper() -> RenderError {
    RenderError::BadCommand("unsupported_shell_wrapper".to_owned())
}

fn is_env_assignment(value: &str) -> bool {
    let Some((name, _)) = value.split_once('=') else {
        return false;
    };
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
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

/// Find one unquoted background operator while keeping chains and redirects.
fn has_background_op(script: &str) -> bool {
    let mut chars = script.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    let mut token_start = true;
    while let Some(ch) = chars.next() {
        if comment {
            if ch == '\n' {
                comment = false;
                token_start = true;
            }
            continue;
        }
        if escaped {
            escaped = false;
            token_start = false;
            continue;
        }
        if let Some(open_quote) = quote {
            if ch == '\\' && open_quote == '"' {
                escaped = true;
            } else if ch == open_quote {
                quote = None;
            }
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '\'' | '"' => {
                quote = Some(ch);
                token_start = false;
            }
            '#' if token_start => comment = true,
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                token_start = true;
                continue;
            }
            '&' if chars.peek() == Some(&'>') => {
                chars.next();
                if chars.peek() == Some(&'>') {
                    chars.next();
                }
                token_start = true;
                continue;
            }
            '&' => return true,
            '<' | '>' if chars.peek() == Some(&'&') => {
                chars.next();
                token_start = true;
                continue;
            }
            '<' if chars.peek() == Some(&'<') => {
                chars.next();
                token_start = true;
                continue;
            }
            '>' if chars.peek() == Some(&'>') => {
                chars.next();
                token_start = true;
                continue;
            }
            ' ' | '\t' | '\n' | '\r' | ';' | '|' | '(' | ')' | '<' | '>' => {
                token_start = true;
            }
            _ => token_start = false,
        }
    }
    false
}
