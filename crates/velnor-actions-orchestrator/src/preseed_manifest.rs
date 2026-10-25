use std::env;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::OrchestratorError;
use crate::internal::internal;

pub const PRESEED_MANIFEST_OP: &str = "write-preseed-manifest-v1";
pub(crate) const PRESEED_BINARY_ENV: &str = "VELNOR_PRESEED_BINARY";
pub(crate) const PRESEED_OUT_ENV: &str = "VELNOR_PRESEED_OUT";
pub(crate) const PRESEED_TARGET_ENV: &str = "VELNOR_PRESEED_TARGET";
pub(crate) const PRESEED_TOOLCHAIN_ENV: &str = "VELNOR_PRESEED_TOOLCHAIN";
const COMMIT_ENV: &str = "GITHUB_SHA";
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

const MANIFEST_FILE: &str = "preseed-manifest.json";

#[derive(Debug, Serialize)]
struct PreseedManifest<'a> {
    schema: u32,
    commit: &'a str,
    target: &'a str,
    toolchain: &'a str,
    sha256: String,
}

pub fn write_preseed_manifest() -> Result<(), OrchestratorError> {
    let binary = nonempty_env(PRESEED_BINARY_ENV)?;
    let out = nonempty_env(PRESEED_OUT_ENV)?;
    let target = nonempty_env(PRESEED_TARGET_ENV)?;
    let toolchain = nonempty_env(PRESEED_TOOLCHAIN_ENV)?;
    let commit = nonempty_env(COMMIT_ENV)?;
    let temp = env::var_os(RUNNER_TEMP_ENV)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("preseed_missing_runner_temp"))?;
    write_preseed_manifest_to(
        &binary,
        &out,
        &target,
        &toolchain,
        &commit,
        Path::new(&temp),
    )
}

pub(crate) fn write_preseed_manifest_to(
    binary: &str,
    out: &str,
    target: &str,
    toolchain: &str,
    commit: &str,
    runner_temp: &Path,
) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(internal(&format!("preseed_unsupported_target:{target}")));
    }
    if !velnor_actions_contract::ids::is_lower_hex_len(commit, 40) {
        return Err(internal("preseed_bad_commit"));
    }
    if toolchain.is_empty() || toolchain.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(internal("preseed_bad_toolchain"));
    }
    let binary_path = Path::new(binary);
    if !binary_path.is_file() {
        return Err(internal("preseed_binary_not_a_file"));
    }
    let out_path = Path::new(out);
    if !(out_path.is_absolute() && out_path.starts_with(runner_temp)) {
        return Err(internal("preseed_outside_runner_temp"));
    }
    let bytes = std::fs::read(binary_path)
        .map_err(|err| OrchestratorError::io(binary.to_owned(), err.to_string()))?;
    let sha256 = hex_lower(Sha256::digest(&bytes).as_slice());
    std::fs::create_dir_all(out_path)
        .map_err(|err| OrchestratorError::io(out.to_owned(), err.to_string()))?;
    let file_name = binary_path
        .file_name()
        .ok_or_else(|| internal("preseed_binary_without_name"))?;
    std::fs::copy(binary_path, out_path.join(file_name))
        .map_err(|err| OrchestratorError::io(out.to_owned(), err.to_string()))?;
    let manifest = PreseedManifest {
        schema: 1,
        commit,
        target,
        toolchain,
        sha256,
    };
    let text = serde_json::to_string(&manifest).map_err(|err| internal(&err.to_string()))?;
    std::fs::write(out_path.join(MANIFEST_FILE), text)
        .map_err(|err| OrchestratorError::io(out.to_owned(), err.to_string()))?;
    Ok(())
}

fn nonempty_env(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("preseed_missing_env:{key}")))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}
