//! Small identity, digest, and environment helpers for the isolated tap writer.

use std::{env, fs, path::Path};

use sha2::{Digest, Sha256};

use crate::{OrchestratorError, internal::internal};

pub(super) fn branch_name(
    receipt_digest: &str,
    base_sha: &str,
) -> Result<String, OrchestratorError> {
    let digest = receipt_digest
        .strip_prefix("b3-")
        .filter(|value| is_sha256(value))
        .ok_or_else(|| internal("tap_writer_branch_identity_invalid"))?;
    if !is_full_sha(base_sha) {
        return Err(internal("tap_writer_branch_identity_invalid"));
    }
    Ok(format!("velnor/preview/{digest}-{base_sha}"))
}

pub(super) fn is_full_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn required_env(key: &str) -> Result<String, OrchestratorError> {
    let value = env::var(key).map_err(|_| internal("tap_writer_env_missing"))?;
    if value.is_empty() || value.len() > 4096 {
        return Err(internal("tap_writer_env_invalid"));
    }
    Ok(value)
}

pub(super) fn parse_decimal_id(value: &str) -> Result<u64, OrchestratorError> {
    let id = value
        .parse::<u64>()
        .map_err(|_| internal("tap_writer_id_invalid"))?;
    if id == 0 || id.to_string() != value {
        return Err(internal("tap_writer_id_invalid"));
    }
    Ok(id)
}

pub(super) fn safe_app_slug(value: &str) -> Result<&str, OrchestratorError> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(internal("tap_writer_app_slug_invalid"));
    }
    Ok(value)
}

pub(super) fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}
