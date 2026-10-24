//! Bounded, failure-only context for CI tool-discovery regressions.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const TOOL_ENV_KEYS: [&str; 11] = crate::impl_cli_tmp::tool_context::TOOL_ENV;
const INHERITED_MISE_SELECTOR_KEYS: [&str; 4] = [
    "MISE_CONFIG_DIR",
    "MISE_CONFIG_FILE",
    "MISE_ENV",
    "MISE_TRUSTED_CONFIG_PATHS",
];
const TOOL_PATH_KEYS: [&str; 12] = [
    "HOME",
    "MISE_DATA_DIR",
    "MISE_INSTALLS_DIR",
    "MISE_CACHE_DIR",
    "MISE_CARGO_HOME",
    "MISE_CONFIG_DIR",
    "MISE_CONFIG_FILE",
    "MISE_RUSTUP_HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
];
#[derive(Clone, Copy)]
pub(crate) enum MiseDiagnosticContext {
    PathOnly,
    PathAndToolHomes,
    Inherited,
}

/// Capture the allowlisted path and fixture metadata without launching tools.
pub(crate) fn mise_path_snapshot(cwd: &Path, context: MiseDiagnosticContext) -> String {
    let mut report = String::new();
    push_line(
        &mut report,
        format_args!("cwd={} context={}", cwd.display(), context_name(context)),
    );
    for key in TOOL_ENV_KEYS {
        let value = match context {
            MiseDiagnosticContext::PathAndToolHomes | MiseDiagnosticContext::Inherited => {
                parent_value(key)
            }
            MiseDiagnosticContext::PathOnly => None,
        };
        push_line(
            &mut report,
            format_args!(
                "{}={}",
                effective_key_name(context, key),
                display_value(value.as_deref())
            ),
        );
        if matches!(context, MiseDiagnosticContext::PathOnly) {
            push_line(
                &mut report,
                format_args!(
                    "parent_{key}={}",
                    display_value(parent_value(key).as_deref())
                ),
            );
        }
        if TOOL_PATH_KEYS.contains(&key)
            && let Some(path) = value.as_deref()
        {
            append_path_metadata(&mut report, key, Path::new(path));
        }
    }
    if matches!(context, MiseDiagnosticContext::Inherited) {
        for key in INHERITED_MISE_SELECTOR_KEYS {
            let value = parent_value(key);
            push_line(
                &mut report,
                format_args!("effective_{key}={}", display_value(value.as_deref())),
            );
        }
    }
    append_fixture_config_metadata(&mut report, cwd);
    let data_dir = data_dir(context);
    if let Some(path) = data_dir {
        append_data_dir_metadata(&mut report, "effective-mise", &path);
    } else {
        push_line(
            &mut report,
            format_args!("effective-mise=<no data-dir/home in diagnostic context>"),
        );
    }
    if matches!(context, MiseDiagnosticContext::PathOnly)
        && let Some(path) = parent_data_dir()
    {
        append_data_dir_metadata(&mut report, "parent-mise", &path);
    }
    report
}

/// Add filesystem and executable metadata after a test failure.
pub(crate) fn mise_failure_diagnostics(
    cwd: &Path,
    context: MiseDiagnosticContext,
    _explicit_env: &[(&str, &str)],
    before: Option<&str>,
) -> String {
    let mut report = String::new();
    if let Some(before) = before {
        push_line(
            &mut report,
            format_args!("tool state before first generation:\n{before}"),
        );
    }
    report.push_str(&mise_path_snapshot(cwd, context));
    append_mise_executable(&mut report);
    report
}

fn append_fixture_config_metadata(report: &mut String, cwd: &Path) {
    for relative in [
        "mise.toml",
        ".mise.toml",
        ".tool-versions",
        "rust-toolchain.toml",
        "rust-toolchain",
        ".mise/config.toml",
        "mise/config.toml",
    ] {
        append_path_metadata(report, "fixture-config", &cwd.join(relative));
    }
}

fn append_data_dir_metadata(report: &mut String, label: &str, data_dir: &Path) {
    for path in [
        data_dir.to_path_buf(),
        data_dir.join("installs"),
        data_dir.join("installs/rust"),
        data_dir.join("installs/rust/1.99.1"),
    ] {
        append_path_metadata(report, label, &path);
    }
}

fn append_path_metadata(report: &mut String, label: &str, path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            let kind = if metadata.file_type().is_symlink() {
                "symlink"
            } else if metadata.is_dir() {
                "directory"
            } else if metadata.is_file() {
                "file"
            } else {
                "other"
            };
            let target = std::fs::read_link(path)
                .map(|target| format!(" target={}", target.display()))
                .unwrap_or_default();
            push_line(
                report,
                format_args!(
                    "{label}={} kind={kind} bytes={}{}",
                    path.display(),
                    metadata.len(),
                    target
                ),
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            push_line(report, format_args!("{label}={} missing", path.display()));
        }
        Err(error) => {
            push_line(
                report,
                format_args!("{label}={} metadata_error={error}", path.display()),
            );
        }
    }
}

fn append_mise_executable(report: &mut String) {
    let Some(path) = std::env::var_os("PATH") else {
        push_line(report, format_args!("mise-executable=<PATH unset>"));
        return;
    };
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join("mise");
        if std::fs::symlink_metadata(&candidate).is_ok() {
            append_path_metadata(report, "mise-executable", &candidate);
            push_line(
                report,
                format_args!(
                    "mise-executable-resolved={}",
                    candidate.canonicalize().unwrap_or(candidate).display()
                ),
            );
            return;
        }
    }
    push_line(report, format_args!("mise-executable=<not found on PATH>"));
}

fn push_line(report: &mut String, arguments: std::fmt::Arguments<'_>) {
    match report.write_fmt(arguments) {
        Ok(()) => report.push('\n'),
        Err(_) => report.push_str("<diagnostic formatting failed>\n"),
    }
}

fn parent_data_dir() -> Option<PathBuf> {
    parent_value("MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| parent_value("XDG_DATA_HOME").map(|path| PathBuf::from(path).join("mise")))
        .or_else(|| parent_value("HOME").map(|path| PathBuf::from(path).join(".local/share/mise")))
}

fn data_dir(context: MiseDiagnosticContext) -> Option<PathBuf> {
    match context {
        MiseDiagnosticContext::PathAndToolHomes | MiseDiagnosticContext::Inherited => {
            parent_data_dir()
        }
        MiseDiagnosticContext::PathOnly => None,
    }
}

fn parent_value(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(key).filter(|value| !value.is_empty())
}

fn display_value(value: Option<&std::ffi::OsStr>) -> String {
    value.map_or_else(
        || "<unset>".to_owned(),
        |value| value.to_string_lossy().into_owned(),
    )
}

fn context_name(context: MiseDiagnosticContext) -> &'static str {
    match context {
        MiseDiagnosticContext::PathOnly => "spawn-isolated-path-only",
        MiseDiagnosticContext::PathAndToolHomes => "spawn-isolated-path-and-tool-homes",
        MiseDiagnosticContext::Inherited => "inherited-verify-policy",
    }
}

fn effective_key_name(context: MiseDiagnosticContext, key: &str) -> String {
    match context {
        MiseDiagnosticContext::PathOnly
        | MiseDiagnosticContext::PathAndToolHomes
        | MiseDiagnosticContext::Inherited => {
            format!("effective_{key}")
        }
    }
}
