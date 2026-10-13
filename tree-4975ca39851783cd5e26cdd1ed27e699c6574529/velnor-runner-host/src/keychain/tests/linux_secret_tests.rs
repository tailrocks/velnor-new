#[cfg(target_os = "linux")]
use crate::HostError;

#[cfg(target_os = "linux")]
#[test]
fn systemd_credential_requires_private_owned_directory_and_file() -> Result<(), HostError> {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDir::new()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Keychain)?;
    let path = directory.path().join("github-token");
    std::fs::write(&path, b"canary-token\n").map_err(|_| HostError::Keychain)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Keychain)?;
    let loaded = super::super::linux::test_load(directory.path(), "github-token")?;
    if loaded.as_slice() != b"canary-token\n" {
        return Err(HostError::Keychain);
    }
    if super::super::linux::test_load(directory.path(), "other-token").is_ok() {
        return Err(HostError::Keychain);
    }

    let actions_token = directory.path().join("actions-read-token");
    std::fs::write(&actions_token, b"read-only-canary\n").map_err(|_| HostError::Keychain)?;
    std::fs::set_permissions(&actions_token, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Keychain)?;
    let actions_loaded = super::super::linux::test_load_actions_read_token(directory.path())?;
    if actions_loaded.as_slice() != b"read-only-canary\n" {
        return Err(HostError::Keychain);
    }
    if super::super::linux::test_load(directory.path(), "../actions-read-token").is_ok()
        || super::super::linux::test_load(directory.path(), "other-token").is_ok()
    {
        return Err(HostError::Keychain);
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .map_err(|_| HostError::Keychain)?;
    if super::super::linux::test_load(directory.path(), "github-token").is_ok() {
        return Err(HostError::Keychain);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn systemd_credential_rejects_symlinked_token() -> Result<(), HostError> {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDir::new()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Keychain)?;
    let target = directory.path().join("target-token");
    std::fs::write(&target, b"canary-token").map_err(|_| HostError::Keychain)?;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Keychain)?;
    std::os::unix::fs::symlink(&target, directory.path().join("github-token"))
        .map_err(|_| HostError::Keychain)?;

    assert!(super::super::linux::test_load(directory.path(), "github-token").is_err());
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn actions_read_credential_rejects_symlink_and_oversized_file() -> Result<(), HostError> {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDir::new()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Keychain)?;
    let target = directory.path().join("target-token");
    std::fs::write(&target, b"canary-token").map_err(|_| HostError::Keychain)?;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Keychain)?;
    std::os::unix::fs::symlink(&target, directory.path().join("actions-read-token"))
        .map_err(|_| HostError::Keychain)?;
    assert!(super::super::linux::test_load_actions_read_token(directory.path()).is_err());

    std::fs::remove_file(directory.path().join("actions-read-token"))
        .map_err(|_| HostError::Keychain)?;
    std::fs::write(
        directory.path().join("actions-read-token"),
        vec![b'x'; 4097],
    )
    .map_err(|_| HostError::Keychain)?;
    std::fs::set_permissions(
        directory.path().join("actions-read-token"),
        std::fs::Permissions::from_mode(0o600),
    )
    .map_err(|_| HostError::Keychain)?;
    assert!(super::super::linux::test_load_actions_read_token(directory.path()).is_err());
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_secret_store_is_limited_to_root_owned_directory() -> Result<(), HostError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let directory = TestDir::new()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Keychain)?;
    let path = directory.path().join("github-token");
    let policy = test_secret_policy(directory.path())?;
    super::super::linux::test_store(&path, b"canary-token", &policy)?;
    let metadata = std::fs::metadata(&path).map_err(|_| HostError::Keychain)?;
    if metadata.mode() & 0o777 != 0o600 {
        return Err(HostError::Keychain);
    }
    if std::fs::read(&path).map_err(|_| HostError::Keychain)? != b"canary-token" {
        return Err(HostError::Keychain);
    }
    if super::super::linux::test_arbitrary_store(&path, b"must-not-use-arbitrary-path").is_ok() {
        return Err(HostError::Keychain);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_secret_removal_is_exact_idempotent_and_rejects_unsafe_files() -> Result<(), HostError> {
    use std::os::unix::fs::PermissionsExt;

    let directory = TestDir::new()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Keychain)?;
    let policy = test_secret_policy(directory.path())?;
    let secret = directory.path().join("github-token");
    std::fs::write(&secret, b"canary-token").map_err(|_| HostError::Keychain)?;
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Keychain)?;

    super::super::linux::test_remove(&secret, &policy)?;
    if secret.exists() {
        return Err(HostError::Keychain);
    }
    super::super::linux::test_remove(&secret, &policy)?;
    if super::super::linux::test_arbitrary_remove(&secret).is_ok() {
        return Err(HostError::Keychain);
    }

    let unsafe_file = directory.path().join("unsafe-token");
    std::fs::write(&unsafe_file, b"canary-token").map_err(|_| HostError::Keychain)?;
    std::fs::set_permissions(&unsafe_file, std::fs::Permissions::from_mode(0o644))
        .map_err(|_| HostError::Keychain)?;
    if super::super::linux::test_remove(&unsafe_file, &policy).is_ok() || !unsafe_file.exists() {
        return Err(HostError::Keychain);
    }

    #[cfg(unix)]
    {
        let symlink = directory.path().join("linked-token");
        std::os::unix::fs::symlink(&unsafe_file, &symlink).map_err(|_| HostError::Keychain)?;
        if super::super::linux::test_remove(&symlink, &policy).is_ok() || !symlink.exists() {
            return Err(HostError::Keychain);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn test_secret_policy(
    directory: &std::path::Path,
) -> Result<super::super::linux::SecretFilePolicy, HostError> {
    use std::os::unix::fs::MetadataExt;

    let directory_metadata = std::fs::metadata(directory).map_err(|_| HostError::Keychain)?;
    let group = std::process::Command::new("/usr/bin/id")
        .arg("-g")
        .output()
        .map_err(|_| HostError::Keychain)?;
    if !group.status.success() {
        return Err(HostError::Keychain);
    }
    let file_gid = String::from_utf8(group.stdout)
        .map_err(|_| HostError::Keychain)?
        .trim()
        .parse::<u32>()
        .map_err(|_| HostError::Keychain)?;
    Ok(super::super::linux::test_policy(
        directory_metadata.uid(),
        directory_metadata.gid(),
        directory_metadata.mode() & 0o7777,
        directory_metadata.uid(),
        file_gid,
        0o600,
    ))
}

#[cfg(target_os = "linux")]
struct TestDir(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl TestDir {
    fn new() -> Result<Self, HostError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-credentials-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).map_err(|_| HostError::Keychain)?;
        Ok(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

#[cfg(target_os = "linux")]
impl Drop for TestDir {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}
