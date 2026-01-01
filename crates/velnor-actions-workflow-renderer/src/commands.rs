//! Fixed command-vector validation and `run:` quoting.
//!
//! Argv arrives as validated vectors; quoting preserves `$` expansion spans
//! for runner variables while quoting every other character.

use crate::{
    RenderError,
    commands_scan::{expansion_end, is_expansion_at, quote_expansion},
    steps::scan_for_private_subcommands,
};

pub(crate) use crate::commands_env::validate_composite_env;
pub use crate::commands_env::validate_env;

/// Validate a fixed argument vector: nonempty, no shell fragments.
///
/// Rejects empty argv, empty args, control characters, command substitution,
/// `cargo install` sequences, absolute Cargo paths, and private tokens.
/// GitHub `${{ }}` expressions stay constructible here: secret and
/// input handles are diagnosed with specific tokens by the release
/// gates, which run after this structural check.
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
        let without_date = arg.replace("$(date +%s%3N)", "");
        if without_date.contains("$(") || arg.contains('`') {
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

/// Join validated argv into one `run:` line with POSIX quoting.
///
/// `$NAME`/`${...}` spans pass through for runner expansion; every other
/// character is quoted. See [`quote_run_arg`]. The inline script of
/// `sh -c`/`bash -c` is single-quoted whole instead: inner-shell
/// variables (assigned or inherited) must survive the outer shell, and
/// the script's own quotes must stay syntactic, not literal. The
/// `env -u` prefixes are transparent to the shape check: a wrapped
/// `sh -c` still quotes its script, at its shifted index.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] when argv validation fails.
pub fn join_argv_for_run(argv: &[String]) -> Result<String, RenderError> {
    validate_command_argv(argv)?;
    let script_at = inline_shell_script_at(argv);
    let mut words = Vec::with_capacity(argv.len());
    for (index, arg) in argv.iter().enumerate() {
        words.push(if script_at == Some(index) {
            quote_script_arg(arg)
        } else {
            quote_run_arg(arg)?
        });
    }
    Ok(words.join(" "))
}

/// True for `sh -c <script>`/`bash -c <script>` vectors.
pub(crate) fn is_inline_shell(argv: &[String]) -> bool {
    argv.len() > 2 && matches!(argv[0].as_str(), "sh" | "bash") && argv[1].as_str() == "-c"
}

fn inline_shell_script_at(argv: &[String]) -> Option<usize> {
    if is_inline_shell(argv) {
        return Some(2);
    }
    if argv.first().is_none_or(|arg| arg != "env") {
        return None;
    }
    let mut command_at = 1;
    while argv.get(command_at).is_some_and(|arg| arg == "-u") && argv.get(command_at + 1).is_some()
    {
        command_at += 2;
    }
    is_inline_shell(argv.get(command_at..)?).then_some(command_at + 2)
}

/// Single-quote one inline script; only `'` needs escaping.
///
/// Inside single quotes `$`, `"`, and `\` stay literal for the outer
/// shell, so the inner shell expands inherited env plus its own
/// assignments exactly as the fixed script intends.
fn quote_script_arg(script: &str) -> String {
    format!("'{}'", script.replace('\'', "'\\''"))
}

/// POSIX-quote one argv element, preserving and protecting `$` expansions.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] when an expansion is malformed or its
/// fallback syntax cannot be preserved safely.
pub fn quote_run_arg(arg: &str) -> Result<String, RenderError> {
    if is_plain_run_token(arg) && !arg.contains('$') {
        return Ok(arg.to_owned());
    }
    let mut out = String::new();
    let mut literal = String::new();
    let bytes = arg.as_bytes();
    let mut offset = 0;
    while offset < bytes.len() {
        if is_github_expression_at(bytes, offset) {
            flush_run_literal(&mut literal, &mut out);
            let end = github_expression_end(bytes, offset)
                .ok_or_else(|| RenderError::BadCommand("unclosed_github_expression".to_owned()))?;
            out.push_str(&arg[offset..end]);
            offset = end;
            continue;
        }
        if bytes[offset] == b'$' && is_expansion_at(bytes, offset) {
            flush_run_literal(&mut literal, &mut out);
            let end = expansion_end(bytes, offset)
                .ok_or_else(|| RenderError::BadCommand("unclosed_shell_expansion".to_owned()))?;
            out.push_str(&quote_expansion(&arg[offset..end])?);
            offset = end;
            continue;
        }
        let ch = arg[offset..]
            .chars()
            .next()
            .ok_or_else(|| RenderError::BadCommand("invalid_utf8_boundary".to_owned()))?;
        if ch == '\'' {
            flush_run_literal(&mut literal, &mut out);
            out.push_str("'\\''");
        } else {
            literal.push(ch);
        }
        offset += ch.len_utf8();
    }
    flush_run_literal(&mut literal, &mut out);
    Ok(out)
}
/// True for `/cargo` or paths ending in `/cargo`.
fn is_absolute_cargo(arg: &str) -> bool {
    arg.starts_with('/') && (arg == "/cargo" || arg.ends_with("/cargo"))
}

/// True for shell background operators (`&`), never for `&&`/redirections.
///
/// Bans ` & `, leading `&`, and trailing single `&`; `&&` chains, `>&`
/// redirections, and URL query `&` stay legal fixed-vector content.
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

/// Flush a pending literal run as one single-quoted span.
fn flush_run_literal(literal: &mut String, out: &mut String) {
    if !literal.is_empty() {
        out.push('\'');
        out.push_str(literal);
        out.push('\'');
        literal.clear();
    }
}

fn is_github_expression_at(bytes: &[u8], index: usize) -> bool {
    bytes.get(index..index + 3) == Some(b"${{")
}

fn github_expression_end(bytes: &[u8], index: usize) -> Option<usize> {
    bytes
        .get(index + 3..)?
        .windows(2)
        .position(|window| window == b"}}")
        .map(|offset| index + 3 + offset + 2)
}

/// Reject Rust invocations outside pinned `mise exec` (RQ-9.3).
///
/// Scans `run:` content only (inline values plus `|`/`>` blocks):
/// every command line naming a Rust program must also name `mise`.
/// Case-sensitive: uppercase env paths (`$CARGO_HOME`) are not
/// invocations, and step names never scan.
/// # Errors
pub fn check_no_bare_cargo(yaml: &str) -> Result<(), RenderError> {
    let mut in_run_block = false;
    let mut run_indent = 0;
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if in_run_block {
            if trimmed.is_empty() || indent <= run_indent {
                in_run_block = false;
            } else {
                check_command_line(trimmed)?;
                continue;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("run:") {
            let rest = rest.trim();
            if rest == "|" || rest == ">" {
                in_run_block = true;
                run_indent = indent;
            } else if !rest.is_empty() {
                check_command_line(rest)?;
            }
        }
    }
    Ok(())
}

/// Reject one command line naming a Rust program without `mise`.
fn check_command_line(command: &str) -> Result<(), RenderError> {
    if command.contains("mise") {
        return Ok(());
    }
    for program in ["cargo", "rustc", "rustup", "mbx", "nextest"] {
        if command.contains(program) {
            return Err(RenderError::BadCommand(format!(
                "bare_rust_invocation:{program}"
            )));
        }
    }
    Ok(())
}
