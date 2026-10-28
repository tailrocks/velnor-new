// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]

include!("../src/entry.rs");

#[derive(Clone, Copy, Eq, PartialEq)]
enum DiagnosticsMode {
    Quiet,
    Control,
    WarningAll,
    NestedOnly,
    DogfoodOnly,
    Fail57,
    ReplaceCopiedGuard,
    ChangeCopiedSource,
}

fn main() -> ExitCode {
    let outcome = run_entry();
    if outcome != ExitCode::SUCCESS || !is_plan_command() {
        return outcome;
    }
    let Some(mode_value) = std::env::var("VELNOR_PLAN_DIAGNOSTICS_MODE").ok() else {
        return outcome;
    };
    let Some(mode) = DiagnosticsMode::parse(&mode_value) else {
        return ExitCode::from(56);
    };
    if let Err(exit) = emit_plan_diagnostics(mode) {
        return exit;
    }
    if mutate_after_plan(mode).is_err() {
        return ExitCode::from(56);
    }
    outcome
}

impl DiagnosticsMode {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "quiet" => Some(Self::Quiet),
            "control" => Some(Self::Control),
            "warning-all" => Some(Self::WarningAll),
            "nested-only" => Some(Self::NestedOnly),
            "dogfood-only" => Some(Self::DogfoodOnly),
            "fail57" => Some(Self::Fail57),
            "replace-copied-guard" => Some(Self::ReplaceCopiedGuard),
            "change-copied-source" => Some(Self::ChangeCopiedSource),
            _ => None,
        }
    }
}

fn is_plan_command() -> bool {
    use std::ffi::OsStr;

    let mut args = std::env::args_os();
    args.len() == 2 && args.nth(1).as_deref() == Some(OsStr::new("plan"))
}

fn emit_plan_diagnostics(mode: DiagnosticsMode) -> Result<(), ExitCode> {
    use std::ffi::OsStr;
    use std::path::Path;

    if mode == DiagnosticsMode::Quiet {
        return Ok(());
    }
    let Some(root) = std::env::current_dir()
        .ok()
        .and_then(|path| path.canonicalize().ok())
    else {
        return Err(ExitCode::from(56));
    };
    let Some(source_root) = std::env::var_os("VELNOR_PLAN_DIAGNOSTICS_ROOT")
        .and_then(|path| Path::new(&path).canonicalize().ok())
    else {
        return Err(ExitCode::from(56));
    };
    let is_nested = root.file_name() == Some(OsStr::new("nested"));
    let is_dogfood = root == source_root;
    eprintln!("Repository: {}", root.display());
    eprintln!("Preview: {}", root.join("diagnostic-preview").display());
    eprintln!(
        "Plan context: repository={} source-root={}",
        root.display(),
        source_root.display()
    );
    let emit_warning = match mode {
        DiagnosticsMode::WarningAll => true,
        DiagnosticsMode::NestedOnly | DiagnosticsMode::Fail57 => is_nested,
        DiagnosticsMode::DogfoodOnly => is_dogfood,
        DiagnosticsMode::Quiet
        | DiagnosticsMode::Control
        | DiagnosticsMode::ReplaceCopiedGuard
        | DiagnosticsMode::ChangeCopiedSource => false,
    };
    if emit_warning {
        eprintln!("warning: plan-diagnostic-regression");
    }
    if mode == DiagnosticsMode::Fail57 && is_nested {
        Err(ExitCode::from(57))
    } else {
        Ok(())
    }
}

fn mutate_after_plan(mode: DiagnosticsMode) -> std::io::Result<()> {
    use std::ffi::OsStr;

    if !matches!(
        mode,
        DiagnosticsMode::ReplaceCopiedGuard | DiagnosticsMode::ChangeCopiedSource
    ) {
        return Ok(());
    }
    let current = std::env::current_dir()?.canonicalize()?;
    if current.file_name() != Some(OsStr::new("nested")) {
        return Ok(());
    }
    let root = private_mutation_root()?;
    let source_root = std::env::var_os("VELNOR_PLAN_DIAGNOSTICS_ROOT")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| std::io::Error::other("diagnostics source root is missing"))?
        .canonicalize()?;
    match mode {
        DiagnosticsMode::ReplaceCopiedGuard => {
            let expected_source = root.join("source").canonicalize()?;
            if source_root != expected_source {
                return Err(std::io::Error::other(
                    "guard fault source root is outside the owned fixture root",
                ));
            }
            replace_copied_guard(&root.join("guard/archive-guard"), &root)
        }
        DiagnosticsMode::ChangeCopiedSource => {
            if source_root != root {
                return Err(std::io::Error::other(
                    "source fault root is outside the owned fixture root",
                ));
            }
            change_copied_source(&root.join("Cargo.toml"), &root)
        }
        _ => Ok(()),
    }
}

#[cfg(unix)]
fn private_mutation_root() -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    let requested = std::env::var_os("VELNOR_PLAN_DIAGNOSTICS_MUTATION_ROOT")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::other("diagnostics mutation root is missing"))?;
    if !requested.is_absolute() {
        return Err(std::io::Error::other(
            "diagnostics mutation root must be absolute",
        ));
    }
    let metadata = std::fs::symlink_metadata(&requested)?;
    let root = std::fs::canonicalize(&requested)?;
    if !metadata.file_type().is_dir()
        || root != requested
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(std::io::Error::other(
            "diagnostics mutation root must be a private real directory owned by the current user",
        ));
    }
    let manifest = std::fs::canonicalize(env!("CARGO_MANIFEST_DIR"))?;
    let workspace = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| std::io::Error::other("CLI workspace root is missing"))?;
    let executable = std::env::current_exe()?.canonicalize()?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(std::fs::canonicalize)
        .transpose()?;
    if overlaps(&root, workspace)
        || executable
            .parent()
            .is_some_and(|path| overlaps(&root, path))
        || target.as_deref().is_some_and(|path| overlaps(&root, path))
    {
        return Err(std::io::Error::other(
            "diagnostics mutation root overlaps the checkout or a build artifact",
        ));
    }
    Ok(root)
}

#[cfg(not(unix))]
fn private_mutation_root() -> std::io::Result<std::path::PathBuf> {
    Err(std::io::Error::other(
        "diagnostics filesystem faults require Unix ownership checks",
    ))
}

#[cfg(unix)]
fn replace_copied_guard(path: &std::path::Path, root: &std::path::Path) -> std::io::Result<()> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

    let metadata = owned_regular_file(path, root, true)?;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("guard parent is missing"))?;
    let parent_metadata = fs::symlink_metadata(parent)?;
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.uid() != rustix::process::geteuid().as_raw()
        || parent_metadata.permissions().mode() & 0o077 != 0
        || fs::canonicalize(parent)? != parent
    {
        return Err(std::io::Error::other(
            "copied guard parent must be a private real directory",
        ));
    }
    let replacement_path = parent.join("archive-guard.replacement");
    let bytes = fs::read(path)?;
    let mut replacement = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(metadata.permissions().mode() & 0o777)
        .open(&replacement_path)?;
    replacement.write_all(&bytes)?;
    replacement.sync_all()?;
    let replacement_metadata = replacement.metadata()?;
    if replacement_metadata.ino() == metadata.ino() {
        return Err(std::io::Error::other(
            "guard replacement did not create a new file identity",
        ));
    }
    drop(replacement);
    fs::rename(replacement_path, path)
}

#[cfg(not(unix))]
fn replace_copied_guard(_path: &std::path::Path, _root: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "guard filesystem fault requires Unix file identities",
    ))
}

#[cfg(unix)]
fn change_copied_source(path: &std::path::Path, root: &std::path::Path) -> std::io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;

    let metadata = owned_regular_file(path, root, false)?;
    let mut file = OpenOptions::new().append(true).open(path)?;
    let opened = file.metadata()?;
    if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
        return Err(std::io::Error::other(
            "copied source manifest changed before mutation",
        ));
    }
    file.write_all(b"\n")?;
    file.sync_all()?;
    let final_metadata = std::fs::symlink_metadata(path)?;
    if final_metadata.dev() != opened.dev() || final_metadata.ino() != opened.ino() {
        return Err(std::io::Error::other(
            "copied source manifest changed during mutation",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn change_copied_source(_path: &std::path::Path, _root: &std::path::Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "source filesystem fault requires Unix file identities",
    ))
}

#[cfg(unix)]
fn owned_regular_file(
    path: &std::path::Path,
    root: &std::path::Path,
    executable: bool,
) -> std::io::Result<std::fs::Metadata> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = std::fs::symlink_metadata(path)?;
    let mode = metadata.permissions().mode();
    if !metadata.file_type().is_file()
        || std::fs::canonicalize(path)? != path
        || !path.starts_with(root)
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.nlink() != 1
        || mode & 0o022 != 0
        || (executable && mode & 0o111 == 0)
    {
        return Err(std::io::Error::other(
            "fault target must be a contained regular file owned by the current user",
        ));
    }
    Ok(metadata)
}

#[cfg(unix)]
fn overlaps(left: &std::path::Path, right: &std::path::Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}
