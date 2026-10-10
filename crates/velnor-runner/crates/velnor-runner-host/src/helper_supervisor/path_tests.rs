use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::{HELPER_NAME, open_regular, open_verified_from, validate_directory};
use crate::error::HostError;

const HELPER_BYTES: &[u8] = b"immutable adjacent verifier fixture";

struct Fixture {
    directory: PathBuf,
    executable: PathBuf,
    helper: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, HostError> {
        let root = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| HostError::Path)?;
        let directory = root.join(format!("velnor-helper-path-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).map_err(|_| HostError::Path)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| HostError::Path)?;
        let executable = directory.join("host");
        write_private_executable(&executable, b"host fixture")?;
        let helper = directory.join(HELPER_NAME);
        write_private_executable(&helper, HELPER_BYTES)?;
        Ok(Self {
            directory,
            executable,
            helper,
        })
    }

    fn expected_sha256() -> [u8; 32] {
        Sha256::digest(HELPER_BYTES).into()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.directory) {
            eprintln!("test helper path cleanup failed: {error}");
        }
    }
}

fn write_private_executable(path: &Path, bytes: &[u8]) -> Result<(), HostError> {
    fs::write(path, bytes).map_err(|_| HostError::Path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| HostError::Path)
}

#[test]
fn opens_only_the_adjacent_helper_with_the_exact_compiled_digest() -> Result<(), HostError> {
    let fixture = Fixture::new()?;
    let mut verified = open_verified_from(&fixture.executable, &Fixture::expected_sha256())?;
    let mut bytes = Vec::new();
    verified
        .file
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Identity)?;
    assert_eq!(bytes, HELPER_BYTES);
    assert_eq!(
        verified.length,
        u64::try_from(HELPER_BYTES.len()).map_err(|_| HostError::Identity)?
    );
    assert_eq!(
        open_verified_from(&fixture.executable, &[0; 32]).err(),
        Some(HostError::Identity)
    );
    Ok(())
}

#[test]
fn rejects_helper_symlinks_and_symlinked_executable_paths() -> Result<(), HostError> {
    let fixture = Fixture::new()?;
    let target = fixture.directory.join("target");
    fs::rename(&fixture.helper, &target).map_err(|_| HostError::Path)?;
    symlink(&target, &fixture.helper).map_err(|_| HostError::Path)?;
    assert_eq!(
        open_verified_from(&fixture.executable, &Fixture::expected_sha256()).err(),
        Some(HostError::Identity)
    );

    let alias = fixture.directory.join("alias");
    symlink(&fixture.directory, &alias).map_err(|_| HostError::Path)?;
    assert_eq!(
        open_verified_from(&alias.join("host"), &Fixture::expected_sha256()).err(),
        Some(HostError::Identity)
    );
    Ok(())
}

#[test]
fn enforces_helper_owner_executable_mode_and_private_parent() -> Result<(), HostError> {
    let fixture = Fixture::new()?;
    let owner = fs::metadata(&fixture.executable)
        .map_err(|_| HostError::Path)?
        .uid();
    assert_eq!(validate_directory(&fixture.directory, owner), Ok(()));
    assert_eq!(
        validate_directory(&fixture.directory, owner ^ 1),
        Err(HostError::Identity)
    );
    assert!(open_regular(&fixture.helper, owner ^ 1).is_err());

    fs::set_permissions(&fixture.helper, fs::Permissions::from_mode(0o777))
        .map_err(|_| HostError::Path)?;
    assert!(open_regular(&fixture.helper, owner).is_err());
    fs::set_permissions(&fixture.helper, fs::Permissions::from_mode(0o600))
        .map_err(|_| HostError::Path)?;
    assert!(open_regular(&fixture.helper, owner).is_err());

    fs::set_permissions(&fixture.helper, fs::Permissions::from_mode(0o700))
        .map_err(|_| HostError::Path)?;
    fs::set_permissions(&fixture.directory, fs::Permissions::from_mode(0o777))
        .map_err(|_| HostError::Path)?;
    assert_eq!(
        validate_directory(&fixture.directory, owner),
        Err(HostError::Identity)
    );
    assert_eq!(
        open_verified_from(&fixture.executable, &Fixture::expected_sha256()).err(),
        Some(HostError::Identity)
    );
    Ok(())
}
