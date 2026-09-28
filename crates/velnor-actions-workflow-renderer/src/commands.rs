//! Fixed command-vector validation and `run:` quoting.
//!
//! Argv arrives as validated vectors; quoting preserves `$` expansion spans
//! for runner variables while quoting every other character.

use std::collections::BTreeMap;

use crate::{RenderError, steps::scan_for_private_subcommands};

/// Validate a fixed argument vector: nonempty, no shell fragments.
///
/// Rejects empty argv, empty args, control characters, command substitution,
/// `cargo install` sequences, absolute Cargo paths, and private tokens.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] or [`RenderError::PrivateSubcommand`].
pub fn validate_command_argv(argv: &[String]) -> Result<(), RenderError> {
    if argv.is_empty() {
        return Err(RenderError::BadCommand("empty_argv".to_owned()));
    }
    let mut previous: Option<&str> = None;
    for arg in argv {
        if arg.is_empty() {
            return Err(RenderError::BadCommand("empty_arg".to_owned()));
        }
        if arg.chars().any(|ch| ch == '\0' || ch == '\n' || ch == '\r') {
            return Err(RenderError::BadCommand(format!("control_char:{arg}")));
        }
        if arg.contains("$(") || arg.contains('`') {
            return Err(RenderError::BadCommand(format!(
                "command_substitution:{arg}"
            )));
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
///
/// Returns [`RenderError::BadCommand`] or [`RenderError::PrivateSubcommand`].
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
        scan_for_private_subcommands(key)?;
        scan_for_private_subcommands(value)?;
    }
    Ok(())
}
/// Join validated argv into one `run:` line with POSIX quoting.
///
/// `$NAME`/`${...}` spans pass through for runner expansion; every other
/// character is quoted. See [`quote_run_arg`].
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] when argv validation fails.
pub fn join_argv_for_run(argv: &[String]) -> Result<String, RenderError> {
    validate_command_argv(argv)?;
    Ok(argv
        .iter()
        .map(|arg| quote_run_arg(arg))
        .collect::<Vec<_>>()
        .join(" "))
}

/// POSIX-quote one argv element, preserving `$` expansion spans.
#[must_use]
pub fn quote_run_arg(arg: &str) -> String {
    if is_plain_run_token(arg) {
        return arg.to_owned();
    }
    let mut out = String::new();
    let mut literal = String::new();
    let mut chars = arg.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' && is_expansion_start(chars.peek()) {
            flush_run_literal(&mut literal, &mut out);
            push_expansion(&mut chars, &mut out);
        } else if ch == '\'' {
            flush_run_literal(&mut literal, &mut out);
            out.push_str("'\\''");
        } else {
            literal.push(ch);
        }
    }
    flush_run_literal(&mut literal, &mut out);
    out
}
/// True for `/cargo` or paths ending in `/cargo`.
fn is_absolute_cargo(arg: &str) -> bool {
    arg.starts_with('/') && (arg == "/cargo" || arg.ends_with("/cargo"))
}
/// True when an argv element needs no quoting in `run:`.
fn is_plain_run_token(arg: &str) -> bool {
    !arg.is_empty()
        && arg.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'$' | b'{'
                        | b'}'
                        | b'_'
                        | b'@'
                        | b'%'
                        | b'+'
                        | b'='
                        | b':'
                        | b','
                        | b'.'
                        | b'/'
                        | b'-'
                )
        })
}

/// True when `$` starts a `$NAME` or `${...}` expansion span.
fn is_expansion_start(next: Option<&char>) -> bool {
    next.is_some_and(|ch| *ch == '{' || ch.is_ascii_alphanumeric() || *ch == '_')
}

/// Copy one `$NAME`/`${...}` span verbatim (braces balanced).
fn push_expansion(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, out: &mut String) {
    out.push('$');
    if chars.peek() == Some(&'{') {
        let mut depth = 0_u32;
        for ch in chars.by_ref() {
            out.push(ch);
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
        }
    } else {
        while let Some(&ch) = chars.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                out.push(ch);
                chars.next();
            } else {
                break;
            }
        }
    }
}

/// Flush a pending literal run as one single-quoted span.
fn flush_run_literal(literal: &mut String, out: &mut String) {
    if !literal.is_empty() {
        out.push('\'');
        out.push_str(literal);
        out.push('\'');
        literal.clear();
    }
}
