use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{Mode, mkfifoat};

use super::{
    DirectoryPolicy, FilePolicy, ModePolicy, open_regular_file_at, open_trusted_directory,
};
use crate::HostError;

#[test]
fn regular_config_read_is_bounded_and_uses_the_open_directory() -> Result<(), HostError> {
    let parent = TestDir::new()?;
    let live = parent.path().join("live");
    fs::create_dir(&live).map_err(|_| HostError::Config)?;
    fs::set_permissions(&live, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Config)?;
    let owner = fs::metadata(&live).map_err(|_| HostError::Config)?;
    let policy = FilePolicy {
        uid: owner.uid(),
        gid: Some(owner.gid()),
        mode: ModePolicy::Exact(0o600),
        min_size: 0,
        max_size: 32,
    };
    fs::write(live.join("host.toml"), "schema = 1\n").map_err(|_| HostError::Config)?;
    fs::set_permissions(live.join("host.toml"), fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Config)?;
    let directory =
        open_trusted_directory(&live, directory_policy(owner.uid(), owner.gid(), 0o700))?;

    let moved = parent.path().join("moved");
    fs::rename(&live, &moved).map_err(|_| HostError::Config)?;
    fs::create_dir(&live).map_err(|_| HostError::Config)?;
    fs::set_permissions(&live, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Config)?;
    fs::write(live.join("host.toml"), "replacement").map_err(|_| HostError::Config)?;
    fs::set_permissions(live.join("host.toml"), fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Config)?;

    let mut original =
        open_regular_file_at(&directory, "host.toml", policy)?.ok_or(HostError::Config)?;
    let mut contents = String::new();
    original
        .read_to_string(&mut contents)
        .map_err(|_| HostError::Config)?;
    assert_eq!(contents, "schema = 1\n");
    fs::remove_dir_all(moved).map_err(|_| HostError::Config)?;
    Ok(())
}

#[test]
fn secure_directory_walk_rejects_symlinks_wrong_owners_and_writable_ancestors()
-> Result<(), HostError> {
    let parent = TestDir::new()?;
    let target = parent.path().join("target");
    fs::create_dir(&target).map_err(|_| HostError::Config)?;
    fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Config)?;
    let metadata = fs::metadata(&target).map_err(|_| HostError::Config)?;
    let expected = directory_policy(metadata.uid(), metadata.gid(), 0o700);
    assert!(open_trusted_directory(&target, expected).is_ok());
    assert!(
        open_trusted_directory(
            &target,
            directory_policy(metadata.uid().saturating_add(1), metadata.gid(), 0o700)
        )
        .is_err()
    );

    let link = parent.path().join("target-link");
    symlink(&target, &link).map_err(|_| HostError::Config)?;
    assert!(open_trusted_directory(&link, expected).is_err());

    let writable = parent.path().join("writable");
    fs::create_dir(&writable).map_err(|_| HostError::Config)?;
    fs::set_permissions(&writable, fs::Permissions::from_mode(0o777))
        .map_err(|_| HostError::Config)?;
    let nested = writable.join("nested");
    fs::create_dir(&nested).map_err(|_| HostError::Config)?;
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Config)?;
    let nested_metadata = fs::metadata(&nested).map_err(|_| HostError::Config)?;
    assert!(
        open_trusted_directory(
            &nested,
            directory_policy(nested_metadata.uid(), nested_metadata.gid(), 0o700)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn file_open_rejects_symlinks_fifos_and_oversized_config() -> Result<(), HostError> {
    let directory = TestDir::new()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Config)?;
    let metadata = fs::metadata(directory.path()).map_err(|_| HostError::Config)?;
    let directory_fd = open_trusted_directory(
        directory.path(),
        directory_policy(metadata.uid(), metadata.gid(), 0o700),
    )?;
    let secret_policy = FilePolicy {
        uid: metadata.uid(),
        gid: None,
        mode: ModePolicy::Clear(0o7077),
        min_size: 1,
        max_size: 4096,
    };

    fs::write(directory.path().join("target"), "canary").map_err(|_| HostError::Config)?;
    fs::set_permissions(
        directory.path().join("target"),
        fs::Permissions::from_mode(0o600),
    )
    .map_err(|_| HostError::Config)?;
    let wrong_owner_policy = FilePolicy {
        uid: metadata.uid().saturating_add(1),
        ..secret_policy
    };
    assert!(open_regular_file_at(&directory_fd, "target", wrong_owner_policy).is_err());
    symlink(
        directory.path().join("target"),
        directory.path().join("github-token"),
    )
    .map_err(|_| HostError::Config)?;
    assert!(open_regular_file_at(&directory_fd, "github-token", secret_policy).is_err());
    fs::remove_file(directory.path().join("github-token")).map_err(|_| HostError::Config)?;

    mkfifoat(&directory_fd, "github-token", Mode::from_raw_mode(0o600))
        .map_err(|_| HostError::Config)?;
    assert!(open_regular_file_at(&directory_fd, "github-token", secret_policy).is_err());
    fs::remove_file(directory.path().join("github-token")).map_err(|_| HostError::Config)?;

    fs::write(directory.path().join("host.toml"), vec![b'x'; 33]).map_err(|_| HostError::Config)?;
    fs::set_permissions(
        directory.path().join("host.toml"),
        fs::Permissions::from_mode(0o600),
    )
    .map_err(|_| HostError::Config)?;
    let config_policy = FilePolicy {
        uid: metadata.uid(),
        gid: Some(metadata.gid()),
        mode: ModePolicy::Exact(0o600),
        min_size: 0,
        max_size: 32,
    };
    assert!(open_regular_file_at(&directory_fd, "host.toml", config_policy).is_err());
    Ok(())
}

#[test]
fn credential_file_helper_returns_the_checked_regular_descriptor() -> Result<(), HostError> {
    let directory = TestDir::new()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Config)?;
    let credential = directory.path().join("github-token");
    fs::write(&credential, b"canary-token").map_err(|_| HostError::Config)?;
    fs::set_permissions(&credential, fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Config)?;
    let uid = fs::metadata(directory.path())
        .map_err(|_| HostError::Config)?
        .uid();
    let mut file =
        super::super::open_systemd_credential_file(directory.path(), "github-token", uid, 4096)?;
    let renamed = directory.path().join("original-token");
    let replacement = directory.path().join("replacement-token");
    fs::write(&replacement, b"replacement").map_err(|_| HostError::Config)?;
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Config)?;
    fs::rename(&credential, &renamed).map_err(|_| HostError::Config)?;
    symlink("replacement-token", &credential).map_err(|_| HostError::Config)?;
    let mut value = String::new();
    file.read_to_string(&mut value)
        .map_err(|_| HostError::Config)?;
    assert_eq!(value, "canary-token");
    assert!(
        super::super::open_systemd_credential_file(directory.path(), "../github-token", uid, 4096,)
            .is_err()
    );
    assert!(
        super::super::open_systemd_credential_file(directory.path(), "other-token", uid, 4096,)
            .is_err()
    );

    let actions_token = directory.path().join("actions-read-token");
    fs::write(&actions_token, b"read-only-canary").map_err(|_| HostError::Config)?;
    fs::set_permissions(&actions_token, fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Config)?;
    let mut actions_file = super::super::open_systemd_credential_file(
        directory.path(),
        "actions-read-token",
        uid,
        4096,
    )?;
    let mut actions_contents = String::new();
    actions_file
        .read_to_string(&mut actions_contents)
        .map_err(|_| HostError::Config)?;
    assert_eq!(actions_contents, "read-only-canary");
    Ok(())
}

fn directory_policy(uid: u32, gid: u32, mode: u32) -> DirectoryPolicy {
    DirectoryPolicy {
        uid,
        gid: Some(gid),
        mode: ModePolicy::Exact(mode),
    }
}

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("velnor-fd-read-{}-{id}", std::process::id()));
        fs::create_dir(&path).map_err(|_| HostError::Config)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
