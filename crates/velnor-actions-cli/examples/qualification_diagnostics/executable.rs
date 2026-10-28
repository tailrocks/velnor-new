// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

pub(super) fn run_version_then_replace_current() -> ExitCode {
    let current = match env::current_exe() {
        Ok(current) => current,
        Err(error) => {
            eprintln!("qualification diagnostics version command failed: {error}");
            return ExitCode::from(56);
        }
    };
    let output = match Command::new(current)
        .arg("--version")
        .env_remove("VELNOR_PLAN_DIAGNOSTICS_MODE")
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            eprintln!("qualification diagnostics version command failed: {error}");
            return ExitCode::from(56);
        }
    };
    if let Err(error) = io::stdout()
        .write_all(&output.stdout)
        .and_then(|()| io::stderr().write_all(&output.stderr))
    {
        eprintln!("qualification diagnostics version output forwarding failed: {error}");
        return ExitCode::from(56);
    }
    if !output.status.success() {
        return output
            .status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from);
    }
    if let Err(error) = replace_current() {
        eprintln!("qualification diagnostics replacement failed: {error}");
        return ExitCode::from(56);
    }
    ExitCode::SUCCESS
}

pub(super) fn replace_external_original() -> io::Result<()> {
    let root = private_mutation_root()?;
    let target = external_target(&root)?;
    replace_executable(&target)
}

pub(super) fn replace_current() -> io::Result<()> {
    let root = private_mutation_root()?;
    let current = fs::canonicalize(env::current_exe()?)?;
    let external = external_target(&root)?;
    if current != external && !collector_executable(&root, &current)? {
        return Err(io::Error::other(
            "current diagnostic executable is outside the admitted fixture paths",
        ));
    }
    replace_executable(&current)
}

pub(super) fn private_mutation_root() -> io::Result<PathBuf> {
    let requested = env::var_os("VELNOR_PLAN_DIAGNOSTICS_MUTATION_ROOT")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("diagnostics mutation root is missing"))?;
    if !requested.is_absolute() {
        return Err(io::Error::other(
            "diagnostics mutation root must be absolute",
        ));
    }
    private_directory(&requested)?;
    let root = fs::canonicalize(&requested)?;
    if root != requested {
        return Err(io::Error::other(
            "diagnostics mutation root must be a canonical directory",
        ));
    }
    let checkout = checkout_root()?;
    let fixture = diagnostic_fixture()?;
    let fixture_parent = fixture
        .parent()
        .ok_or_else(|| io::Error::other("diagnostics fixture parent is missing"))?;
    let current = fs::canonicalize(env::current_exe()?)?;
    let current_parent = current
        .parent()
        .ok_or_else(|| io::Error::other("current executable parent is missing"))?;
    let target = build_target(&checkout)?;
    let current_is_collector_copy =
        overlaps(&root, current_parent) && collector_executable(&root, &current)?;
    if overlaps(&root, &checkout)
        || overlaps(&root, fixture_parent)
        || (overlaps(&root, current_parent) && !current_is_collector_copy)
        || overlaps(&root, &target)
    {
        return Err(io::Error::other(
            "diagnostics mutation root overlaps the checkout, fixture, or build artifacts",
        ));
    }
    Ok(root)
}

fn diagnostic_fixture() -> io::Result<PathBuf> {
    let path = env::var_os("VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("diagnostics fixture locator is missing"))?;
    if !path.is_absolute() {
        return Err(io::Error::other(
            "diagnostics fixture path must be absolute",
        ));
    }
    if fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(io::Error::other(
            "diagnostics fixture path must not be a symlink",
        ));
    }
    let path = fs::canonicalize(path)?;
    executable_file(&path)?;
    Ok(path)
}

fn external_target(root: &Path) -> io::Result<PathBuf> {
    let requested = env::var_os("VELNOR_PLAN_DIAGNOSTICS_EXECUTABLE_TARGET")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("diagnostics executable target is missing"))?;
    if !requested.is_absolute() {
        return Err(io::Error::other(
            "diagnostics executable target must be absolute",
        ));
    }
    let target = fs::canonicalize(&requested)?;
    if target != requested || target == diagnostic_fixture()? {
        return Err(io::Error::other(
            "diagnostics executable target is not the owned fixture candidate",
        ));
    }
    let metadata = executable_file(&target)?;
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("diagnostics executable parent is missing"))?;
    private_directory(parent)?;
    let root_parent = root
        .parent()
        .ok_or_else(|| io::Error::other("diagnostics mutation root parent is missing"))?;
    let checkout = checkout_root()?;
    let fixture = diagnostic_fixture()?;
    let fixture_parent = fixture
        .parent()
        .ok_or_else(|| io::Error::other("diagnostics fixture parent is missing"))?;
    let target_dir = build_target(&checkout)?;
    if parent == root
        || parent.parent() != Some(root_parent)
        || overlaps(root, parent)
        || overlaps(root, &checkout)
        || overlaps(root, fixture_parent)
        || overlaps(root, &target_dir)
        || overlaps(&target_dir, parent)
        || metadata.nlink() != 1
    {
        return Err(io::Error::other(
            "diagnostics executable target is outside its private sibling fixture",
        ));
    }
    Ok(target)
}

fn collector_executable(root: &Path, path: &Path) -> io::Result<bool> {
    let Some(cli_directory) = path.parent() else {
        return Ok(false);
    };
    let Some(workspace) = cli_directory.parent() else {
        return Ok(false);
    };
    if path.file_name().is_none_or(|name| name != "velnor-actions")
        || cli_directory.file_name().is_none_or(|name| name != "cli")
        || workspace.parent() != Some(root)
        || !workspace
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("velnor-goldens-work."))
    {
        return Ok(false);
    }
    private_directory(workspace)?;
    private_directory(cli_directory)?;
    executable_file(path)?;
    Ok(true)
}

fn checkout_root() -> io::Result<PathBuf> {
    let manifest = fs::canonicalize(env!("CARGO_MANIFEST_DIR"))?;
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("CLI workspace root is missing"))
}

fn build_target(checkout: &Path) -> io::Result<PathBuf> {
    match env::var_os("CARGO_TARGET_DIR") {
        Some(path) => {
            let path = PathBuf::from(path);
            fs::canonicalize(if path.is_absolute() {
                path
            } else {
                checkout.join(path)
            })
        }
        None => fs::canonicalize(checkout.join("target")),
    }
}

fn private_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir()
        || fs::canonicalize(path)? != path
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "diagnostics path must be a private directory owned by the current user",
        ));
    }
    Ok(())
}

fn executable_file(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    let mode = metadata.permissions().mode();
    if !metadata.file_type().is_file()
        || fs::canonicalize(path)? != path
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.nlink() != 1
        || mode & 0o022 != 0
        || mode & 0o111 == 0
    {
        return Err(io::Error::other(
            "diagnostics target must be a single-link executable owned by the current user",
        ));
    }
    Ok(metadata)
}

fn replace_executable(path: &Path) -> io::Result<()> {
    let metadata = executable_file(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("executable parent is missing"))?;
    private_directory(parent)?;
    let replacement_path =
        parent.join(format!("velnor-actions.replacement.{}", std::process::id()));
    let mut replacement = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(metadata.permissions().mode() & 0o777)
        .open(&replacement_path)?;
    let result = (|| {
        replacement.write_all(b"not a valid executable\n")?;
        replacement.sync_all()?;
        let replacement_metadata = replacement.metadata()?;
        if replacement_metadata.ino() == metadata.ino() {
            return Err(io::Error::other(
                "diagnostics replacement did not create a new file identity",
            ));
        }
        drop(replacement);
        let current = fs::symlink_metadata(path)?;
        if current.dev() != metadata.dev() || current.ino() != metadata.ino() {
            return Err(io::Error::other(
                "diagnostics target changed before atomic replacement",
            ));
        }
        fs::rename(&replacement_path, path)
    })();
    if result.is_err() {
        drop(fs::remove_file(replacement_path));
    }
    result
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}
