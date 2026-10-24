//! Allowlisted tool context for tests that exercise live installed-tool checks.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub(crate) const TOOL_ENV: [&str; 11] = [
    "HOME",
    "MISE_DATA_DIR",
    "MISE_INSTALLS_DIR",
    "MISE_CACHE_DIR",
    "MISE_CARGO_HOME",
    "MISE_RUSTUP_HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
];

/// Spawn an isolated CLI child while preserving only host tool-install paths.
pub(crate) fn spawn_isolated_with_tools(
    args: &[&str],
    env: &[(&str, &str)],
    cwd: &Path,
) -> Result<Output, Box<dyn std::error::Error>> {
    let mut selected = BTreeMap::<OsString, OsString>::new();
    if let Some(path) = std::env::var_os("PATH") {
        selected.insert(OsString::from("PATH"), path);
    }
    for key in TOOL_ENV {
        if let Some(value) = std::env::var_os(key).filter(|value| !value.is_empty()) {
            selected.insert(OsString::from(key), value);
        }
    }
    for (key, value) in env {
        selected.insert(OsString::from(key), OsString::from(value));
    }

    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command.args(args).current_dir(cwd).env_clear();
    command.envs(selected);
    let output = command.output()?;
    report_missing_pin(&output, cwd, super::MiseDiagnosticContext::PathAndToolHomes);
    Ok(output)
}

/// Preserve child output and report host paths when a live pin check fails.
pub(crate) fn report_missing_pin(
    output: &Output,
    cwd: &Path,
    context: super::MiseDiagnosticContext,
) {
    if !output.status.success()
        && String::from_utf8_lossy(&output.stderr).contains("missing_tool_pin")
    {
        let diagnostics = super::mise_failure_diagnostics(cwd, context, &[], None);
        eprintln!(
            "CLI missing-tool failure in {}; filesystem context:\n{}",
            cwd.display(),
            diagnostics,
        );
    }
}

/// Capture context-aware filesystem diagnostics for the current directory.
pub(crate) fn diagnostic() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    super::mise_failure_diagnostics(&cwd, super::MiseDiagnosticContext::Inherited, &[], None)
}
