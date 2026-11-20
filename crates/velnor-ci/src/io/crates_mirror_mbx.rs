// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::CiError;

use super::crates_mirror::failed;

pub(super) const ENV: &str = "VELNOR_CARGO_MBX";
pub(super) const VALUE: &str = "required";

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
    let directory = project_directory(&work, args);
    let directory = std::fs::canonicalize(&directory).unwrap_or(directory);
    scope(&directory, workspace().as_deref())
        .into_iter()
        .any(crate::mbx_declared::declared_in)
}

fn project_directory(work: &Path, args: &[OsString]) -> PathBuf {
    manifest(args)
        .and_then(|manifest| work.join(manifest).parent().map(Path::to_path_buf))
        .unwrap_or_else(|| work.to_path_buf())
}

fn manifest(args: &[OsString]) -> Option<OsString> {
    let mut args = args.iter().take_while(|arg| *arg != "--");
    while let Some(arg) = args.next() {
        if arg == "--manifest-path" {
            return args.next().cloned();
        }
        if let Some(value) = arg
            .to_str()
            .and_then(|arg| arg.strip_prefix("--manifest-path="))
        {
            return Some(value.into());
        }
    }
    None
}

fn workspace() -> Option<PathBuf> {
    let workspace = std::env::var_os("GITHUB_WORKSPACE").filter(|value| !value.is_empty())?;
    std::fs::canonicalize(workspace).ok()
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
