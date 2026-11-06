// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CheckDeadline, MetadataExt, OrchestratorError, Path, PermissionsExt, freeze_checkout, internal,
};

pub(super) fn stage_baseline(
    view: &Path,
    checksum: &str,
    files: &std::collections::BTreeMap<String, (u32, Vec<u8>)>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    use std::{
        io::Write as _,
        os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _},
    };
    let invalid = || internal("crate_release_source_baseline_invalid");
    if checksum.len() != 64
        || !checksum
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || files.is_empty()
    {
        return Err(invalid());
    }
    deadline.remaining().map_err(|_| invalid())?;
    let parent = view.join("baselines");
    match std::fs::DirBuilder::new().mode(0o700).create(&parent) {
        Ok(()) => std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755))
            .map_err(|_| invalid())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(invalid()),
    }
    let parent_meta = std::fs::symlink_metadata(&parent).map_err(|_| invalid())?;
    if !parent_meta.is_dir()
        || parent_meta.file_type().is_symlink()
        || parent_meta.uid() != 0
        || parent_meta.mode() & 0o022 != 0
    {
        return Err(invalid());
    }
    let root = parent.join(checksum);
    let created = match std::fs::DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(_) => return Err(invalid()),
    };
    if created {
        for (relative, (mode, bytes)) in files {
            deadline.remaining().map_err(|_| invalid())?;
            let output = root.join(relative);
            let containing = output.parent().ok_or_else(invalid)?;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(containing)
                .map_err(|_| invalid())?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&output)
                .map_err(|_| invalid())?;
            for chunk in bytes.chunks(32768) {
                deadline.remaining().map_err(|_| invalid())?;
                file.write_all(chunk).map_err(|_| invalid())?;
            }
            file.set_permissions(std::fs::Permissions::from_mode(if *mode == 0 {
                0o444
            } else {
                0o555
            }))
            .map_err(|_| invalid())?;
        }
        freeze_checkout(&root, deadline)?;
    }
    verify_baseline(&root, files, deadline)
}

fn verify_baseline(
    root: &Path,
    files: &std::collections::BTreeMap<String, (u32, Vec<u8>)>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    use std::collections::BTreeSet;
    let invalid = || internal("crate_release_source_baseline_invalid");
    let root_meta = std::fs::symlink_metadata(root).map_err(|_| invalid())?;
    if !root_meta.is_dir()
        || root_meta.file_type().is_symlink()
        || root_meta.uid() != 0
        || root_meta.mode() & 0o022 != 0
    {
        return Err(invalid());
    }
    let mut expected_dirs = BTreeSet::new();
    for name in files.keys() {
        let mut parent_path = Path::new(name).parent();
        while let Some(path) = parent_path {
            if path.as_os_str().is_empty() {
                break;
            }
            expected_dirs.insert(path.to_path_buf());
            parent_path = path.parent();
        }
    }
    let mut seen_files = BTreeSet::new();
    let mut seen_dirs = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).map_err(|_| invalid())? {
            deadline.remaining().map_err(|_| invalid())?;
            let path = entry.map_err(|_| invalid())?.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid())?
                .to_path_buf();
            let metadata = std::fs::symlink_metadata(&path).map_err(|_| invalid())?;
            if metadata.uid() != 0
                || metadata.dev() != root_meta.dev()
                || metadata.mode() & 0o022 != 0
                || metadata.file_type().is_symlink()
            {
                return Err(invalid());
            }
            if metadata.is_dir() {
                seen_dirs.insert(relative);
                pending.push(path);
            } else if metadata.is_file() && metadata.nlink() == 1 {
                let name = relative.to_str().ok_or_else(invalid)?;
                let (mode, expected) = files.get(name).ok_or_else(invalid)?;
                if (metadata.mode() & 0o111 != 0) != (*mode != 0)
                    || metadata.len() != expected.len() as u64
                    || std::fs::read(&path).map_err(|_| invalid())? != *expected
                {
                    return Err(invalid());
                }
                seen_files.insert(name.to_owned());
            } else {
                return Err(invalid());
            }
        }
    }
    if seen_dirs != expected_dirs || seen_files.len() != files.len() {
        return Err(invalid());
    }
    deadline.remaining().map_err(|_| invalid())?;
    Ok(())
}
