//! Allowlisted tool context for tests that exercise live installed-tool checks.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use velnor_actions_mise::{IsolatedCommand, ProcessOutput};

const TOOL_ENV: &[&str] = &[
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
const RUST_PIN: &str = "1.99.1";
const MAX_CAPTURE_BYTES: usize = 8 * 1024;

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
        if let Some(value) = std::env::var_os(key) {
            selected.insert(OsString::from(key), value);
        }
    }
    for (key, value) in env {
        selected.insert(OsString::from(key), OsString::from(value));
    }

    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-actions"));
    command.args(args).current_dir(cwd).env_clear();
    command.envs(selected);
    Ok(command.output()?)
}

/// Capture real, non-secret environment and Mise inventory facts for failures.
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
            output.push_str("live `mise --no-config --no-env --no-hooks ls --json rust`:\n");
            output.push_str(&mise_inventory(&mise));
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

fn mise_inventory(mise: &Path) -> String {
    match run_diagnostic_command(
        mise,
        &[
            "--no-config",
            "--no-env",
            "--no-hooks",
            "ls",
            "--json",
            "rust",
        ],
    ) {
        Ok(result) => format!("  {}\n", describe_result(&result)),
        Err(error) => format!("  could not query live tool inventory: {error}\n"),
    }
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
            if executable == "rustup" {
                append_rustup_components(output, &path)?;
            }
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

fn append_rustup_components(output: &mut String, rustup: &Path) -> fmt::Result {
    output.push_str(
        "  direct rustup probe uses RUSTUP_HOME (or $HOME/.rustup when unset); MISE_RUSTUP_HOME is not interpreted by rustup\n",
    );
    let actual_rustup = match std::fs::canonicalize(rustup) {
        Ok(path) => path,
        Err(error) => {
            writeln!(output, "  cannot resolve rustup binary: {error}")?;
            return Ok(());
        }
    };
    match run_diagnostic_command(
        &actual_rustup,
        &["component", "list", "--installed", "--toolchain", RUST_PIN],
    ) {
        Ok(result) => {
            writeln!(
                output,
                "  rustup component inventory: {}",
                describe_result(&result)
            )?;
        }
        Err(error) => writeln!(
            output,
            "  could not query installed Rustup components: {error}"
        )?,
    }
    Ok(())
}

fn run_diagnostic_command(program: &Path, payload_args: &[&str]) -> Result<ProcessOutput, String> {
    let mut argv = vec![OsString::from("-i")];
    if let Some(path) = std::env::var_os("PATH") {
        argv.push(env_assignment("PATH", &path));
    }
    for key in TOOL_ENV {
        if let Some(value) = std::env::var_os(key) {
            argv.push(env_assignment(key, &value));
        }
    }
    argv.push(OsString::from("MISE_AUTO_INSTALL=false"));
    argv.push(OsString::from("MISE_EXEC_AUTO_INSTALL=false"));
    argv.push(program.as_os_str().to_owned());
    argv.extend(payload_args.iter().map(OsString::from));
    let command = IsolatedCommand::repo_task("/usr/bin/env", argv, &[])
        .map_err(|error| format!("could not construct isolated probe: {error}"))?;
    command
        .run_bounded(MAX_CAPTURE_BYTES, Duration::from_secs(10))
        .map_err(|error| format!("bounded isolated probe failed: {error}"))
}

fn env_assignment(key: &str, value: &OsStr) -> OsString {
    let mut assignment = OsString::from(key);
    assignment.push("=");
    assignment.push(value);
    assignment
}

fn describe_result(result: &ProcessOutput) -> String {
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    format!(
        "success={} code={:?} signal={:?} stdout={stdout:?} stderr={stderr:?}",
        result.success, result.code, result.signal
    )
}
