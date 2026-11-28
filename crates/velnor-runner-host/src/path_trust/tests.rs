// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use super::{Policy, find, untrusted_path};

type Fixture = Result<(), Box<dyn std::error::Error>>;

const STICKY_ROOT: &str = if cfg!(target_os = "macos") {
    "/private/tmp"
} else {
    "/tmp"
};

fn me() -> u32 {
    rustix::process::geteuid().as_raw()
}

fn policy() -> Policy {
    Policy {
        installer: me(),
        admin_gid: u32::MAX,
    }
}

fn chmod(path: &Path, mode: u32) -> std::io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

fn directory(path: &Path, mode: u32) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(path)?;
    chmod(path, mode)?;
    Ok(path.to_path_buf())
}

fn orb(directory: &Path, mode: u32) -> std::io::Result<PathBuf> {
    let path = directory.join("orb");
    std::fs::write(&path, "#!/bin/sh\n")?;
    chmod(&path, mode)?;
    Ok(path)
}

fn root(temp: &tempfile::TempDir) -> std::io::Result<PathBuf> {
    temp.path().canonicalize()
}

struct Link(PathBuf);

impl Drop for Link {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}

#[test]
fn a_private_orb_is_trusted() -> Fixture {
    let temp = tempfile::tempdir()?;
    let bin = directory(&root(&temp)?.join("bin"), 0o755)?;
    let path = orb(&bin, 0o755)?;
    assert_eq!(find(&path, policy()), None);
    assert_eq!(untrusted_path(&path, me()), None);
    Ok(())
}

#[test]
fn a_group_writable_orb_is_refused_by_name() -> Fixture {
    let temp = tempfile::tempdir()?;
    let path = orb(&directory(&root(&temp)?.join("bin"), 0o755)?, 0o775)?;
    assert_eq!(find(&path, policy()), Some(path));
    Ok(())
}

#[test]
fn an_orb_in_a_world_writable_directory_is_refused_by_name() -> Fixture {
    let temp = tempfile::tempdir()?;
    let bin = directory(&root(&temp)?.join("bin"), 0o777)?;
    let path = orb(&bin, 0o755)?;
    assert_eq!(find(&path, policy()), Some(bin));
    Ok(())
}

#[test]
fn an_orb_owned_by_another_user_is_refused() -> Fixture {
    let temp = tempfile::tempdir()?;
    let path = orb(&directory(&root(&temp)?.join("bin"), 0o755)?, 0o755)?;
    let other = Policy {
        installer: me().wrapping_add(1),
        admin_gid: u32::MAX,
    };
    let refused = find(&path, other).ok_or("accepted")?;
    assert!(path.starts_with(refused));
    Ok(())
}

#[test]
fn a_sticky_directory_not_owned_by_root_is_refused() -> Fixture {
    let temp = tempfile::tempdir()?;
    let shared = directory(&root(&temp)?.join("shared"), 0o1777)?;
    let path = orb(&shared, 0o755)?;
    assert_eq!(find(&path, policy()), Some(shared));
    Ok(())
}

#[test]
fn an_entry_in_a_root_owned_sticky_directory_must_belong_to_root_or_the_installer() -> Fixture {
    let temp = tempfile::tempdir()?;
    let path = orb(&directory(&root(&temp)?.join("bin"), 0o755)?, 0o755)?;
    let link = Link(Path::new(STICKY_ROOT).join(format!("velnor-trust-{}", std::process::id())));
    symlink(&path, &link.0)?;
    assert_eq!(find(&link.0, policy()), None);
    let other = Policy {
        installer: me().wrapping_add(1),
        admin_gid: u32::MAX,
    };
    assert_eq!(find(&link.0, other), Some(link.0.clone()));
    Ok(())
}

#[test]
fn a_two_hop_file_symlink_through_a_writable_directory_is_refused() -> Fixture {
    let temp = tempfile::tempdir()?;
    let root = root(&temp)?;
    let bin = directory(&root.join("bin"), 0o755)?;
    let real = bin.join("real");
    std::fs::write(&real, "#!/bin/sh\n")?;
    chmod(&real, 0o755)?;
    let open = directory(&root.join("open"), 0o777)?;
    symlink(&real, open.join("hop"))?;
    symlink(open.join("hop"), bin.join("orb"))?;
    assert_eq!(find(&bin.join("orb"), policy()), Some(open));
    Ok(())
}

#[test]
fn a_two_hop_directory_symlink_through_a_writable_directory_is_refused() -> Fixture {
    let temp = tempfile::tempdir()?;
    let root = root(&temp)?;
    let bin = directory(&root.join("bin"), 0o755)?;
    orb(&bin, 0o755)?;
    let open = directory(&root.join("open"), 0o777)?;
    symlink(&bin, open.join("hop"))?;
    let outer = directory(&root.join("outer"), 0o755)?;
    symlink(open.join("hop"), outer.join("d"))?;
    assert_eq!(find(&outer.join("d/orb"), policy()), Some(open));
    Ok(())
}

#[test]
fn a_symlink_loop_is_refused() -> Fixture {
    let temp = tempfile::tempdir()?;
    let dir = directory(&root(&temp)?.join("bin"), 0o755)?;
    symlink(dir.join("b"), dir.join("a"))?;
    symlink(dir.join("a"), dir.join("b"))?;
    assert!(find(&dir.join("a"), policy()).is_some());
    Ok(())
}

#[test]
fn a_directory_group_writable_by_admin_is_accepted_and_by_any_other_group_refused() -> Fixture {
    let temp = tempfile::tempdir()?;
    let bin = directory(&root(&temp)?.join("bin"), 0o775)?;
    let path = orb(&bin, 0o755)?;
    let gid = std::fs::metadata(&bin)?.gid();
    let admin = Policy {
        installer: me(),
        admin_gid: gid,
    };
    assert_eq!(find(&path, admin), None);
    let other = Policy {
        installer: me(),
        admin_gid: gid.wrapping_add(1),
    };
    assert_eq!(find(&path, other), Some(bin));
    Ok(())
}

#[cfg(target_os = "macos")]
mod acl {
    use std::path::Path;
    use std::process::Command;

    use super::{Fixture, directory, find, orb, policy, root};

    fn allow(path: &Path, rule: &str) -> std::io::Result<()> {
        let status = Command::new("/bin/chmod")
            .args(["+a", rule])
            .arg(path)
            .status()?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| std::io::ErrorKind::PermissionDenied.into())
    }

    #[test]
    fn an_acl_that_lets_anyone_write_the_orb_is_refused() -> Fixture {
        let temp = tempfile::tempdir()?;
        let path = orb(&directory(&root(&temp)?.join("bin"), 0o755)?, 0o555)?;
        allow(&path, "everyone allow write,append")?;
        assert_eq!(find(&path, policy()), Some(path));
        Ok(())
    }

    #[test]
    fn an_acl_that_lets_anyone_add_files_to_a_directory_is_refused() -> Fixture {
        let temp = tempfile::tempdir()?;
        let bin = directory(&root(&temp)?.join("bin"), 0o755)?;
        let path = orb(&bin, 0o555)?;
        super::chmod(&bin, 0o555)?;
        allow(&bin, "everyone allow add_file,delete_child")?;
        assert_eq!(find(&path, policy()), Some(bin));
        Ok(())
    }

    #[test]
    fn a_deny_only_acl_is_accepted() -> Fixture {
        let temp = tempfile::tempdir()?;
        let bin = directory(&root(&temp)?.join("bin"), 0o755)?;
        let path = orb(&bin, 0o555)?;
        allow(&bin, "everyone deny delete")?;
        assert_eq!(find(&path, policy()), None);
        Ok(())
    }
}
