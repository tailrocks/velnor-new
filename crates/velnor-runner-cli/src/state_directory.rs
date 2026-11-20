// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags};

pub(crate) fn user_home() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    home.is_absolute().then_some(home)
}

pub(crate) fn create_private_dir_all(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(path)?;
    let leaf = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    rustix::fs::fchmod(&leaf, Mode::from_raw_mode(0o700)).map_err(std::io::Error::from)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::create_private_dir_all;

    fn mode(path: &std::path::Path) -> std::io::Result<u32> {
        Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
    }

    #[test]
    fn an_existing_open_directory_is_tightened_to_private() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let state = temp.path().join("state");
        std::fs::create_dir(&state)?;
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755))?;
        create_private_dir_all(&state)?;
        assert_eq!(mode(&state)?, 0o700);
        Ok(())
    }

    #[test]
    fn a_symlinked_leaf_is_refused_and_its_target_is_untouched() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("target");
        std::fs::create_dir(&target)?;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
        let link = temp.path().join("state");
        std::os::unix::fs::symlink(&target, &link)?;
        assert!(create_private_dir_all(&link).is_err());
        assert_eq!(mode(&target)?, 0o755);
        Ok(())
    }
}
