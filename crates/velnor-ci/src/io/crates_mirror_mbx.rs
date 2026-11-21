// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::CiError;

use super::crates_mirror::failed;

pub(super) const ENV: &str = "VELNOR_CARGO_MBX";
pub(super) const VALUE: &str = "required";
const MANIFEST_COMMANDS: &[&str] = &[
    "add",
    "b",
    "bench",
    "build",
    "c",
    "check",
    "clean",
    "clippy",
    "d",
    "doc",
    "fetch",
    "fix",
    "generate-lockfile",
    "locate-project",
    "metadata",
    "package",
    "pkgid",
    "publish",
    "r",
    "read-manifest",
    "remove",
    "rm",
    "run",
    "rustc",
    "rustdoc",
    "t",
    "test",
    "tree",
    "update",
    "vendor",
    "verify-project",
];

pub(super) fn required(args: &[OsString]) -> bool {
    recorded() && !nested_in_cargo() && declared_for_project(args)
}

fn nested_in_cargo() -> bool {
    std::env::var_os("CARGO_MANIFEST_DIR").is_some()
}

fn recorded() -> bool {
    std::env::var_os(ENV).is_some_and(|value| !value.is_empty())
        || super::crates_mirror_mode::recorded().is_ok_and(|mode| mode.is_some_and(|mode| mode.mbx))
}

fn declared_for_project(args: &[OsString]) -> bool {
    let Ok(work) = std::env::current_dir() else {
        return true;
    };
    let Ok(workspace) = workspace() else {
        return true;
    };
    let Some(directory) = project_directory(&work, args) else {
        return true;
    };
    let directory = std::fs::canonicalize(&directory).unwrap_or(directory);
    scope(&directory, workspace.as_deref())
        .into_iter()
        .any(crate::mbx_declared::declared_or_unreadable_in)
}

fn project_directory(work: &Path, args: &[OsString]) -> Option<PathBuf> {
    let base = directory_flag(work, args);
    let args = super::crates_mirror_alias::expand(&base, args, MANIFEST_COMMANDS)?;
    let command = subcommand(&args).map(|(_, command)| command);
    if let Some(target) = command.filter(|command| direct_target(command)) {
        let script = base.join(target);
        return Some(
            script
                .parent()
                .map_or_else(|| base.clone(), Path::to_path_buf),
        );
    }
    if command == Some("install")
        && let Some(path) = option_value(&args, "--path", false)
    {
        return Some(base.join(path));
    }
    let shorthand = command.is_some_and(|command| MANIFEST_COMMANDS.contains(&command));
    Some(
        option_value(&args, "--manifest-path", shorthand)
            .and_then(|manifest| base.join(manifest).parent().map(Path::to_path_buf))
            .unwrap_or_else(|| base.clone()),
    )
}

fn directory_flag(work: &Path, args: &[OsString]) -> PathBuf {
    let mut base = work.to_path_buf();
    let mut seen = args.iter().take_while(|arg| *arg != "--");
    while let Some(arg) = seen.next() {
        let Some(text) = arg.to_str() else {
            break;
        };
        if text == "-C" {
            if let Some(directory) = seen.next() {
                base = base.join(directory);
            }
        } else if let Some(directory) = text.strip_prefix("-C") {
            base = base.join(directory);
        } else if matches!(text, "-Z" | "--config" | "--color") {
            seen.next();
        } else if !text.starts_with(['-', '+']) {
            break;
        }
    }
    base
}

fn direct_target(command: &str) -> bool {
    let path = Path::new(command);
    path.extension().is_some_and(|extension| extension == "rs")
        || path.file_name().is_some_and(|name| name == "Cargo.toml")
}

pub(super) fn subcommand(args: &[OsString]) -> Option<(usize, &str)> {
    let mut seen = args.iter().enumerate().take_while(|(_, arg)| *arg != "--");
    while let Some((index, arg)) = seen.next() {
        let arg = arg.to_str()?;
        if matches!(arg, "-C" | "-Z" | "--config" | "--color") {
            seen.next();
        } else if !arg.starts_with(['-', '+']) {
            return Some((index, arg));
        }
    }
    None
}

pub(super) fn option_value(args: &[OsString], long: &str, shorthand: bool) -> Option<OsString> {
    let mut args = args.iter().take_while(|arg| *arg != "--");
    while let Some(arg) = args.next() {
        if arg == long || (shorthand && arg == "-m") {
            return args.next().cloned();
        }
        let Some(arg) = arg.to_str() else {
            continue;
        };
        if let Some(value) = arg
            .strip_prefix(long)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return Some(value.into());
        }
        if shorthand
            && !arg.starts_with("--")
            && let Some(value) = arg.strip_prefix("-m")
        {
            return Some(value.strip_prefix('=').unwrap_or(value).into());
        }
    }
    None
}

fn workspace() -> std::io::Result<Option<PathBuf>> {
    std::env::var_os("GITHUB_WORKSPACE")
        .filter(|value| !value.is_empty())
        .map(|path| {
            let path = std::fs::canonicalize(path)?;
            if path.is_dir() {
                Ok(path)
            } else {
                Err(std::io::ErrorKind::NotADirectory.into())
            }
        })
        .transpose()
}

fn scope<'a>(directory: &'a Path, workspace: Option<&Path>) -> Vec<&'a Path> {
    match workspace {
        Some(workspace) if directory.starts_with(workspace) => directory
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(workspace))
            .collect(),
        _ => vec![directory],
    }
}

pub(super) fn refused(cargo: &Path) -> CiError {
    CiError::new(format!(
        "mbx is declared but cargo on PATH isn't the mise mbx wrapper: {}",
        cargo.display()
    ))
}

pub(super) fn wrapper_missing() -> CiError {
    CiError::new("mbx is declared but mise's cargo wrapper is missing after activation")
}

pub(super) fn declared_here() -> Result<bool, CiError> {
    let directory = std::env::current_dir().map_err(|error| failed(&error))?;
    crate::mbx_declared::checked_in(&directory).map_err(CiError::new)
}

pub(super) fn export_line(declared: bool) -> String {
    format!("{ENV}={}\n", if declared { VALUE } else { "" })
}
