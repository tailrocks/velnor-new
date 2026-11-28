// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const GROUP_OR_OTHER_WRITE: u32 = 0o022;
const STICKY: u32 = 0o1000;

pub(super) fn untrusted(path: &Path, installer: u32) -> Option<PathBuf> {
    let Ok(real) = std::fs::canonicalize(path) else {
        return Some(path.to_path_buf());
    };
    if !file_is_trusted(&real, installer) {
        return Some(real);
    }
    path.ancestors()
        .skip(1)
        .chain(real.ancestors().skip(1))
        .find(|directory| !directory_is_trusted(directory, installer))
        .map(Path::to_path_buf)
}

fn owned_by_trusted_user(uid: u32, installer: u32) -> bool {
    uid == 0 || uid == installer
}

fn file_is_trusted(path: &Path, installer: u32) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| {
        metadata.is_file()
            && owned_by_trusted_user(metadata.uid(), installer)
            && metadata.mode() & GROUP_OR_OTHER_WRITE == 0
    })
}

fn directory_is_trusted(path: &Path, installer: u32) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| {
        let sticky_root = metadata.uid() == 0 && metadata.mode() & STICKY != 0;
        owned_by_trusted_user(metadata.uid(), installer)
            && (metadata.mode() & GROUP_OR_OTHER_WRITE == 0 || sticky_root)
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::untrusted;

    fn me() -> u32 {
        rustix::process::geteuid().as_raw()
    }

    fn orb(path: &Path, mode: u32) -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().ok_or(std::io::ErrorKind::InvalidInput)?)?;
        std::fs::write(path, "#!/bin/sh\n")?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    #[test]
    fn a_private_orb_is_trusted() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().canonicalize()?.join("bin/orb");
        orb(&path, 0o755)?;
        assert_eq!(untrusted(&path, me()), None);
        Ok(())
    }

    #[test]
    fn a_group_writable_orb_is_refused_by_name() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().canonicalize()?.join("bin/orb");
        orb(&path, 0o775)?;
        assert_eq!(untrusted(&path, me()), Some(path));
        Ok(())
    }

    #[test]
    fn an_orb_in_a_world_writable_directory_is_refused_by_name() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().canonicalize()?.join("bin/orb");
        orb(&path, 0o755)?;
        let directory = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o777))?;
        assert_eq!(untrusted(&path, me()), Some(directory.to_path_buf()));
        Ok(())
    }

    #[test]
    fn an_orb_owned_by_another_user_is_refused_by_name() -> std::io::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().canonicalize()?.join("bin/orb");
        orb(&path, 0o755)?;
        assert_eq!(untrusted(&path, me().wrapping_add(1)), Some(path));
        Ok(())
    }
}
