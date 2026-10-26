// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::RenderError;

pub(crate) const JQ_VERSION_PATTERN: &str = r"jq-1\.8\.[0-9]+";

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

pub(crate) fn capture_command(producer: &str, file: &str, variable: &str) -> String {
    format!(
        "{producer} > \"{file}\"; IFS= read -r {variable} < \"{file}\"; test -n \"${variable}\""
    )
}

pub(crate) fn jq_guard() -> String {
    format!(
        "{}; test -x \"$jq_path\"; \"$jq_path\" --version | /usr/bin/grep -Eqx '{JQ_VERSION_PATTERN}'",
        capture_command("command -v jq", "$task_root/jq_path.txt", "jq_path")
    )
}
