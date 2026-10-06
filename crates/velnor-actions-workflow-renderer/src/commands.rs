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

/// Validate a fixed env map: `A-Z0-9_` keys, single-line clean values.
///
/// Expressions stay allowlisted, never blanket-banned: only fixed
/// runner-provided spans pass (see the private `expressions` module).
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
        crate::expressions::check_env_value(key, value)?;
        scan_for_private_subcommands(key)?;
        scan_for_private_subcommands(value)?;
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
/// credential-unset prefix (shared `env -u` length predicate) is
/// transparent to the shape check: a wrapped `sh -c` still quotes
/// its script, at its shifted index.
///
/// # Errors
///
/// Returns [`RenderError::BadCommand`] when argv validation fails.
pub fn join_argv_for_run(argv: &[String]) -> Result<String, RenderError> {
    validate_command_argv(argv)?;
    let prefix = crate::toolchain_env::unset_prefix_len(argv);
    let script_at = is_inline_shell(&argv[prefix..]).then_some(prefix + 2);
    Ok(argv
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            if script_at == Some(index) {
                quote_script_arg(arg)
            } else {
                quote_run_arg(arg)
            }
        })
        .collect::<Vec<_>>()
        .join(" "))
}

/// True for `sh -c <script>`/`bash -c <script>` vectors.
pub(crate) fn is_inline_shell(argv: &[String]) -> bool {
    argv.len() > 2 && matches!(argv[0].as_str(), "sh" | "bash") && argv[1].as_str() == "-c"
}

/// Single-quote one inline script; only `'` needs escaping.
///
/// Inside single quotes `$`, `"`, and `\` stay literal for the outer
/// shell, so the inner shell expands inherited env plus its own
/// assignments exactly as the fixed script intends.
fn quote_script_arg(script: &str) -> String {
    format!("'{}'", script.replace('\'', "'\\''"))
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

/// Double-quote an env-derived shell path for `run:` lines.
#[must_use]
pub fn quote_env_path_for_run(path: &str) -> String {
    let quoted = path.len() >= 2 && path.starts_with('"') && path.ends_with('"');
    if quoted || !has_shell_expansion(path) {
        return path.to_owned();
    }
    format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
}

/// True when a `run:` line carries a bare `$VAR/` word-splitting pattern.
#[must_use]
pub fn has_bare_env_expansion(line: &str) -> bool {
    scan_run_words(line).iter().any(|(_, bare)| *bare)
}

/// Quote bare env words in a joined `run:` line (idempotent).
///
/// Words that already carry quoting are never rewritten: converting
/// single-quoted `'...'$VAR'...'` concatenation to double quotes would
/// expand inner-shell variables (e.g. `read`-assigned `$sha`) in the
/// outer shell instead. Only fully bare words gain double quotes.
#[must_use]
pub fn quote_run_line_env_paths(run_line: &str) -> String {
    scan_run_words(run_line)
        .into_iter()
        .map(|(word, bare)| {
            if bare && !word.contains('\'') && !word.contains('"') {
                quote_env_path_for_run(&decode_run_word(&word))
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Split a `run:` line into words plus bare-expansion flags.
fn scan_run_words(run_line: &str) -> Vec<(String, bool)> {
    let bytes = run_line.as_bytes();
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut bare = false;
    let mut quote = 0u8;
    let mut chars = run_line.char_indices();
    while let Some((index, ch)) = chars.next() {
        if quote == 1 {
            cur.push(ch);
            if ch == '\'' {
                quote = 0;
            }
        } else if quote == 2 {
            cur.push(ch);
            if ch == '\\' {
                cur.extend(chars.next().map(|(_, next)| next));
            } else if ch == '"' {
                quote = 0;
            }
        } else if ch == '\'' {
            quote = 1;
            cur.push(ch);
        } else if ch == '"' {
            quote = 2;
            cur.push(ch);
        } else if ch.is_whitespace() {
            if !cur.is_empty() {
                words.push((std::mem::take(&mut cur), std::mem::take(&mut bare)));
            }
        } else {
            bare = bare || ch == '$' && is_shell_expansion_at(bytes, index);
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        words.push((cur, bare));
    }
    words
}

/// Decode single-quote spans (`'\''` → `'`).
fn decode_run_word(word: &str) -> String {
    word.replace("'\\''", "\0")
        .replace('\'', "")
        .replace('\0', "'")
}

/// True when text holds a `$NAME`/`${NAME}` shell expansion span.
fn has_shell_expansion(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'$' && is_shell_expansion_at(bytes, index))
}

/// True when `bytes[index] == b'$'` starts a shell span (`${{` excluded).
fn is_shell_expansion_at(bytes: &[u8], index: usize) -> bool {
    match bytes.get(index + 1) {
        Some(b'{') => bytes.get(index + 2) != Some(&b'{'),
        Some(next) => next.is_ascii_alphabetic() || *next == b'_',
        None => false,
    }
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
