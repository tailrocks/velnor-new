//! Shared single-line guard-script builders for typed task jobs.
//!
//! Captures use the `> file` plus `read` idiom and file writes use `printf`,
//! so emitted scripts stay free of newlines, command substitution, and
//! backticks per the renderer's command policy.

use crate::RenderError;

pub(crate) const JQ_VERSION: &str = "jq-1.8.2";

/// Write exact `content` lines with `printf`, failing closed on unsafe bytes.
///
/// Every line stays printable ASCII without carriage returns; the generator
/// owns the content, so anything else is a bug, never user input. Lines
/// without a single quote stay literal inside single quotes; lines with one
/// use escaped double quotes. `scope` names the calling task kind in errors.
pub(crate) fn printf_write(path: &str, content: &str, scope: &str) -> Result<String, RenderError> {
    if content.is_empty() {
        return Err(RenderError::InvalidWorkflow(format!(
            "{scope}_empty_mise_file"
        )));
    }
    let content = content.strip_suffix('\n').unwrap_or(content);
    let mut command = vec!["printf '%s\\n'".to_owned()];
    for line in content.split('\n') {
        if line.bytes().any(|byte| !(0x20..=0x7e).contains(&byte)) {
            return Err(RenderError::InvalidWorkflow(format!(
                "{scope}_mise_file_bytes"
            )));
        }
        if line.contains('\'') {
            command.push(format!("\"{}\"", shell_escape_double(line)));
        } else {
            command.push(format!("'{line}'"));
        }
    }
    Ok(format!("{} > \"{path}\"", command.join(" ")))
}

/// Escape one `printf` data argument for POSIX double quotes.
fn shell_escape_double(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'\\' | b'"' | b'$' | b'`' => {
                escaped.push('\\');
                escaped.push(byte as char);
            }
            _ => escaped.push(byte as char),
        }
    }
    escaped
}

/// Run `producer`, capturing its exact first line into `variable`.
///
/// `read` keeps the whole line; a missing or empty capture fails the guard.
pub(crate) fn capture_command(producer: &str, file: &str, variable: &str) -> String {
    format!(
        "{producer} > \"{file}\"; IFS= read -r {variable} < \"{file}\"; test -n \"${variable}\""
    )
}

/// Resolve `jq` to an exact pinned version before reading Mise config.
pub(crate) fn jq_guard() -> String {
    format!(
        "{}; test -x \"$jq_path\"; \"$jq_path\" --version | /usr/bin/grep -Fqx '{JQ_VERSION}'",
        capture_command("command -v jq", "$task_root/jq_path.txt", "jq_path")
    )
}
