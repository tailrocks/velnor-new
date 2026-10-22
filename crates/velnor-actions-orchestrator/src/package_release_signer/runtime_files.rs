use crate::{OrchestratorError, internal::internal};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};
use velnor_actions_contract::package_release::PackageReleaseReceipt;

pub(in crate::package_release_signer) fn validate_release_directory(
    directory: &Path,
    receipt: &PackageReleaseReceipt,
) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| internal("package_release_directory_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("package_release_directory_invalid"));
    }
    let expected = receipt
        .assets
        .iter()
        .map(|asset| asset.name.as_str())
        .chain(["release.json", "release.json.bundle"])
        .collect::<BTreeSet<_>>();
    let mut observed = BTreeSet::new();
    for entry in
        fs::read_dir(directory).map_err(|_| internal("package_release_directory_read_failed"))?
    {
        let entry = entry.map_err(|_| internal("package_release_directory_read_failed"))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| internal("package_release_asset_name_invalid"))?;
        if !expected.contains(name.as_str()) || observed.len() >= expected.len() {
            return Err(internal("package_release_directory_inventory_mismatch"));
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| internal("package_release_asset_invalid"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() || !observed.insert(name) {
            return Err(internal("package_release_directory_entry_invalid"));
        }
    }
    if observed.len() != expected.len() || !expected.iter().all(|name| observed.contains(*name)) {
        return Err(internal("package_release_directory_inventory_mismatch"));
    }
    Ok(())
}

pub(in crate::package_release_signer) fn read_regular_file(
    path: &Path,
    max_bytes: u64,
) -> Result<Vec<u8>, OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("package_signer_file_missing"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > max_bytes
    {
        return Err(internal("package_signer_file_invalid"));
    }
    fs::read(path).map_err(|_| internal("package_signer_file_read_failed"))
}

pub(in crate::package_release_signer) fn require_private_directory(
    path: &Path,
) -> Result<(), OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("package_signer_directory_missing"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("package_signer_directory_invalid"));
    }
    Ok(())
}

pub(in crate::package_release_signer) fn absolute_env(
    name: &str,
) -> Result<PathBuf, OrchestratorError> {
    let path = PathBuf::from(required_env(name)?);
    if !path.is_absolute() {
        return Err(internal("package_signer_path_not_absolute"));
    }
    Ok(path)
}

pub(in crate::package_release_signer) fn required_env(
    name: &str,
) -> Result<String, OrchestratorError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("package_signer_context_missing"))
}

pub(in crate::package_release_signer) fn positive_u64(
    name: &str,
) -> Result<u64, OrchestratorError> {
    required_env(name)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_signer_run_identity_invalid"))
}

pub(in crate::package_release_signer) fn positive_u32(
    name: &str,
) -> Result<u32, OrchestratorError> {
    required_env(name)?
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| internal("package_signer_run_identity_invalid"))
}

pub(in crate::package_release_signer) fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(in crate::package_release_signer) fn write_new_file(
    path: &Path,
    bytes: &[u8],
) -> Result<(), OrchestratorError> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| internal("package_signer_create_failed"))?;
    file.write_all(bytes)
        .map_err(|_| internal("package_signer_write_failed"))?;
    file.sync_all()
        .map_err(|_| internal("package_signer_sync_failed"))
}

pub(in crate::package_release_signer) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}
