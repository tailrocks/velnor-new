//! Event-time `write-preseed-manifest-v1`: fresh helper writes its own manifest.
//!
//! Bootstrap paradox, resolved: the manifest verifies the helper before
//! any consumer executes it, so the plan job cannot stage-then-verify.
//! Writing is not embedding — the bootstrap contract (§2) bans only a
//! compile-time bake of a manifest into the asset it describes, and §4
//! bars the candidate from deciding its own graph, not from describing
//! its own bytes. The plan job builds the helper from source, then runs
//! the FRESH binary here: it hashes its own file, copies itself beside
//! the manifest, and writes the §4.4 JSON through serde — manifest bytes
//! are produced by Rust, never shell-composed.
//!
//! The download-side verification stays shell by necessity, not choice:
//! consumers must verify the manifest BEFORE staging, and pre-staging
//! they hold no trusted executor (executing downloaded bytes to verify
//! them would defeat verification). See the renderer's contract
//! exception note on `preseed_manifest_verify_script`.
//!
//! Renderer side: `preseed_manifest_verify_script` in
//! `velnor-actions-workflow-jobs`.

use std::env;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;

/// Manifest-writing operation tag.
pub const PRESEED_MANIFEST_OP: &str = "write-preseed-manifest-v1";
/// Env key carrying the fresh binary path (build output, relative ok).
pub const PRESEED_BINARY_ENV: &str = "VELNOR_PRESEED_BINARY";
/// Env key carrying the expanded manifest output directory.
pub const PRESEED_OUT_ENV: &str = "VELNOR_PRESEED_OUT";
/// Env key carrying the literal target triple.
pub const PRESEED_TARGET_ENV: &str = "VELNOR_PRESEED_TARGET";
/// Env key carrying the toolchain identity derived from the build vector.
pub const PRESEED_TOOLCHAIN_ENV: &str = "VELNOR_PRESEED_TOOLCHAIN";
/// Env key carrying the source commit (ambient GitHub SHA).
const COMMIT_ENV: &str = "GITHUB_SHA";
/// Env key scoping the output directory (unexpanded expressions rejected).
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

/// Pre-seed manifest filename inside the uploaded artifact.
///
/// Pinned equal to the renderer's `PRESEED_MANIFEST_FILE` by test: the
/// writer and the uploader/download sides must name one file.
pub const MANIFEST_FILE: &str = "preseed-manifest.json";

/// §4.4 manifest shape: schema, commit, target, toolchain, digest.
///
/// Field order is the serialized order, matching the verification
/// parser's expectations exactly (no pretty printing, no newline).
#[derive(Debug, Serialize)]
struct PreseedManifest<'a> {
    /// Manifest schema; always 1.
    schema: u32,
    /// Recorded source commit (40 lowercase hex).
    commit: &'a str,
    /// Build target triple.
    target: &'a str,
    /// Toolchain identity from the fixed build vector.
    toolchain: &'a str,
    /// SHA-256 of the helper binary (64 lowercase hex).
    sha256: String,
}

/// Hash the fresh binary, stage the copy, and write its manifest.
///
/// All inputs arrive via env (see `PRESEED_*_ENV` plus ambient
/// `GITHUB_SHA`/`RUNNER_TEMP`); the output directory must be absolute
/// and under runner temp, so an unexpanded `${{ }}` expression fails
/// closed instead of writing outside the artifact root.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing or invalid env,
/// unsupported targets, unreadable binaries, and unwritable outputs.
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

/// Write the manifest for explicit inputs (env already resolved).
///
/// Split from [`write_preseed_manifest`] so tests drive the writer
/// without process-global env (forbidden by `unsafe_code`).
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for invalid inputs and
/// [`OrchestratorError::Io`] for filesystem failures.
pub fn write_preseed_manifest_to(
    binary: &str,
    out: &str,
    target: &str,
    toolchain: &str,
    commit: &str,
    runner_temp: &Path,
) -> Result<(), OrchestratorError> {
    if !velnor_actions_contract_release::is_supported_target(target) {
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

/// Require one nonempty env value.
fn nonempty_env(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("preseed_missing_env:{key}")))
}

/// Lowercase hex encoding of digest bytes.
fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}
