//! Shared single-line guard-script builders for typed task jobs.
//!
//! Captures use the `> file` plus `read` idiom and file writes use `printf`,
//! so emitted scripts stay free of newlines, command substitution, and
//! backticks per the renderer's command policy.

use crate::RenderError;

/// Accepted `jq --version` output: any 1.8 patch release.
///
/// `jq` only parses `mise config ls --json` before tools are installed, so it
/// is a host dependency, not an installed tool. GitHub-hosted images ship
/// `jq` patch releases that move independently of this crate (`ubuntu-26.04`
/// ships 1.8.1), so an exact patch pin made every task job fail there. The
/// minor line is pinned because the `-r '.[].path'` filter and the `--version`
/// format are the contract; see `docs/implemented/host-jq-range.md`.
pub(crate) const JQ_VERSION_PATTERN: &str = r"jq-1\.8\.[0-9]+";

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

/// Resolve `jq` to an absolute executable on the accepted 1.8 line before reading Mise config.
pub(crate) fn jq_guard() -> String {
    format!(
        "{}; test -x \"$jq_path\"; \"$jq_path\" --version | /usr/bin/grep -Eqx '{JQ_VERSION_PATTERN}'",
        capture_command("command -v jq", "$task_root/jq_path.txt", "jq_path")
    )
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use super::jq_guard;

    /// Run the real emitted guard against a fake `jq` printing `reported`.
    fn guard_accepts(reported: &str) -> bool {
        let root = std::env::temp_dir().join(format!(
            "velnor-jq-guard-{}-{}",
            std::process::id(),
            reported.replace(['/', ' '], "_")
        ));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("fake bin dir");
        let jq = bin.join("jq");
        std::fs::write(&jq, format!("#!/bin/sh\necho '{reported}'\n")).expect("fake jq");
        std::fs::set_permissions(&jq, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let script = format!(
            "set -euo pipefail; task_root='{}'; {}",
            root.display(),
            jq_guard()
        );
        let status = Command::new("bash")
            .args(["-c", &script])
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .status()
            .expect("bash runs");
        std::fs::remove_dir_all(&root).expect("cleanup");
        status.success()
    }

    /// `ubuntu-26.04` ships jq 1.8.1; the exact `jq-1.8.2` pin rejected it.
    #[test]
    fn guard_accepts_every_jq_1_8_patch_release() {
        for version in ["jq-1.8.0", "jq-1.8.1", "jq-1.8.2", "jq-1.8.10"] {
            assert!(guard_accepts(version), "{version} must be accepted");
        }
    }

    #[test]
    fn guard_rejects_other_minor_lines_and_lookalikes() {
        for version in [
            "jq-1.7.1",
            "jq-1.9.0",
            "jq-2.8.1",
            "jq-1.8",
            "jq-1.8.1-dirty",
            "jq-1x8.1",
            "xjq-1.8.1",
        ] {
            assert!(!guard_accepts(version), "{version} must be rejected");
        }
    }
}
