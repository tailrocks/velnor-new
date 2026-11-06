// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use tempfile::Builder;

use super::super::{CollectorError, filesystem};
use super::{ensure_real_directory, remove_existing};

pub(super) fn capture_goldens(
    golden_dir: &Path,
    stage: &Path,
    dogfood_identical: bool,
) -> Result<(), CollectorError> {
    if !dogfood_identical {
        return Err(CollectorError::mismatch(
            "dogfood generated workflows are not identical to repository workflows",
        ));
    }
    ensure_real_directory(golden_dir)?;
    let parent = golden_dir
        .parent()
        .ok_or_else(|| CollectorError::operational("golden directory has no parent"))?;
    let metadata = fs::symlink_metadata(golden_dir).map_err(CollectorError::operational)?;
    let temporary = Builder::new()
        .prefix(".opentofu-goldens-stage.")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)
        .map_err(CollectorError::operational)?;
    let replacement = temporary.path().join("replacement");
    fs::create_dir(&replacement).map_err(CollectorError::operational)?;
    filesystem::copy_contents(golden_dir, &replacement)?;
    let cases = replacement.join("cases");
    remove_existing(&cases)?;
    fs::create_dir(&cases).map_err(CollectorError::operational)?;
    filesystem::copy_contents(stage, &cases)?;
    let tree = filesystem::read_tree(&cases)?;
    fs::write(
        replacement.join("MANIFEST.sha256"),
        filesystem::tree_hash(&tree),
    )
    .map_err(CollectorError::operational)?;
    fs::set_permissions(&replacement, metadata.permissions())
        .map_err(CollectorError::operational)?;
    exchange_directories(&replacement, golden_dir).map_err(CollectorError::operational)?;
    println!("wrote {}", golden_dir.join("cases").display());
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exchange_directories(staged: &Path, target: &Path) -> std::io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, staged, CWD, target, RenameFlags::EXCHANGE).map_err(std::io::Error::from)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn exchange_directories(_staged: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic directory exchange is unsupported on this platform",
    ))
}
