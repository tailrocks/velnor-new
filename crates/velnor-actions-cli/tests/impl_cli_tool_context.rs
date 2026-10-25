//! Allowlisted tool context for tests that exercise live installed-tool checks.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub(crate) const TOOL_ENV: [&str; 13] = [
    "HOME",
    "MISE_DATA_DIR",
    "MISE_INSTALLS_DIR",
    "MISE_CACHE_DIR",
    "MISE_STATE_DIR",
    "MISE_CARGO_HOME",
    "MISE_RUSTUP_HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
];
const RUST_PIN: &str = "1.99.1";

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
    report_missing_pin(
        &output,
        cwd,
        super::MiseDiagnosticContext::PathAndToolHomes,
        env,
    );
    Ok(output)
}

/// Preserve child output and report host paths when a live pin check fails.
pub(crate) fn report_missing_pin(
    output: &Output,
    cwd: &Path,
    context: super::MiseDiagnosticContext,
    explicit_env: &[(&str, &str)],
) {
    if !output.status.success()
        && String::from_utf8_lossy(&output.stderr).contains("missing_tool_pin")
    {
        let diagnostics = super::mise_failure_diagnostics(cwd, context, explicit_env, None);
        eprintln!(
            "CLI missing-tool failure in {}; filesystem context:\n{}",
            cwd.display(),
            diagnostics,
        );
    }
}

/// Capture real, non-secret environment and installed-tool paths for failures.
pub(crate) fn diagnostic() -> String {
    match diagnostic_text() {
        Ok(output) => output,
        Err(error) => format!("tool context diagnostics unavailable: {error:?}\n"),
    }
}

fn diagnostic_text() -> Result<String, fmt::Error> {
    let mut output = String::from("allowlisted tool context:\n");
    for key in TOOL_ENV {
        let value = std::env::var_os(key).map_or_else(
            || "<unset>".to_owned(),
            |value| value.to_string_lossy().into_owned(),
        );
        writeln!(output, "  {key}={value}")?;
    }

    let path = std::env::var_os("PATH").unwrap_or_default();
    let mise = find_mise(&path);
    match mise {
        Some(mise) => {
            writeln!(output, "resolved mise: {}", describe_path(&mise))?;
        }
        None => output.push_str("resolved mise: <not found on PATH>\n"),
    }
    append_rust_install_status(&mut output)?;
    Ok(output)
}

fn find_mise(path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|directory| directory.join("mise"))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| std::fs::canonicalize(candidate).ok())
}

fn append_rust_install_status(output: &mut String) -> fmt::Result {
    let installs_dir = env_path("MISE_INSTALLS_DIR").or_else(|| {
        env_path("MISE_DATA_DIR")
            .or_else(|| env_path("XDG_DATA_HOME").map(|path| path.join("mise")))
            .or_else(|| env_path("HOME").map(|path| path.join(".local/share/mise")))
            .map(|path| path.join("installs"))
    });
    if let Some(installs_dir) = installs_dir {
        let install = installs_dir.join("rust").join(RUST_PIN);
        writeln!(output, "Rust install {}", describe_path(&install))?;
        for executable in ["rustc", "cargo", "rustup"] {
            let path = install.join(executable);
            writeln!(output, "  {}", describe_path(&path))?;
        }
    } else {
        output.push_str("Rust install path: <cannot derive from allowlisted paths>\n");
    }

    for key in ["MISE_RUSTUP_HOME", "RUSTUP_HOME"] {
        if let Some(home) = env_path(key) {
            let toolchains = home.join("toolchains");
            writeln!(
                output,
                "{key} toolchains {}: {}",
                toolchains.display(),
                path_kind(&toolchains)
            )?;
            append_toolchain_entries(output, &toolchains)?;
        }
    }
    Ok(())
}

fn append_toolchain_entries(output: &mut String, toolchains: &Path) -> fmt::Result {
    let entries = match std::fs::read_dir(toolchains) {
        Ok(entries) => entries,
        Err(error) => {
            writeln!(output, "  cannot read toolchain entries: {error}")?;
            return Ok(());
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                writeln!(output, "  cannot inspect toolchain entry: {error}")?;
                continue;
            }
        };
        if !entry.file_name().to_string_lossy().contains(RUST_PIN) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        writeln!(
            output,
            "  Rustup toolchain directory {name}: {}",
            describe_path(&path)
        )?;
        for component in ["bin/rustc", "bin/cargo", "lib/rustlib/components"] {
            let component_path = path.join(component);
            writeln!(output, "    {component}: {}", path_kind(&component_path))?;
        }
    }
    Ok(())
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).map(PathBuf::from)
}

fn path_kind(path: &Path) -> &'static str {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => "symlink",
        Ok(metadata) if metadata.is_dir() => "directory",
        Ok(metadata) if metadata.is_file() => "file",
        Ok(_) => "other",
        Err(_) => "missing",
    }
}

fn describe_path(path: &Path) -> String {
    let kind = path_kind(path);
    let target = std::fs::read_link(path)
        .map(|target| format!(" -> {}", target.display()))
        .unwrap_or_default();
    format!("{} ({kind}){target}", path.display())
}
