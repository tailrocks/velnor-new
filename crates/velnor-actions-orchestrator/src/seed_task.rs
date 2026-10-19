//! `resolve-seed-task-v1`: one seed lane record as an `env`-ready NUL frame.
//!
//! Selected by full execution digest from the checked-in manifest, which is
//! validated exactly as `resolve-task-execution-v1` validates it (marker,
//! strict canonical JSON, record digests), but without a plan binding: the
//! Cache seed runs outside the CI run. Only build, Clippy, and doc records
//! resolve, and only for the lane that names their kind. The frame is
//! `KEY=value` for every record env entry followed by the argv, each
//! NUL-terminated, with machine-output flags inserted right after the Cargo
//! subcommand (they do not change unit fingerprints):
//!
//! - build (`nextest list`): `--cargo-message-format json-render-diagnostics`;
//!   build (`test`/`build`): `--message-format json-render-diagnostics`;
//! - Clippy and doc: `--message-format json-render-diagnostics --keep-going`.

use std::path::Path;

use velnor_actions_contract::workflow::cache_family::SeedLane;
use velnor_actions_contract::{MAX_TASK_EXECUTION_FRAME_BYTES, validate_digest};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::task_execution::{
    read_manifest, resolve_runner_temp_expression, unique_record_by_execution_digest,
    validate_runner_temp_path,
};

const JSON_DIAGNOSTICS: &str = "json-render-diagnostics";

/// Resolve the selected record from the runner environment.
/// # Errors
///
/// Returns an internal error for missing runner context, an invalid
/// manifest, an unknown digest, or a record outside the lane.
pub fn resolve_seed_task() -> Result<Vec<u8>, OrchestratorError> {
    let root = super::absolute_env("GITHUB_WORKSPACE")?;
    let runner_temp = super::absolute_env("RUNNER_TEMP")?;
    let digest = required("VELNOR_TASK_EXECUTION_DIGEST")?;
    let version = required("VELNOR_GENERATOR_VERSION")?;
    let lane = required("VELNOR_SEED_LANE")?;
    resolve_seed_task_to(&root, &runner_temp, &digest, &version, &lane)
}

fn required(key: &str) -> Result<String, OrchestratorError> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal(&format!("missing_{}", key.to_ascii_lowercase())))
}

/// Resolve one record of `lane` by its execution digest.
/// # Errors
///
/// Same as [`resolve_seed_task`].
pub(crate) fn resolve_seed_task_to(
    root: &Path,
    runner_temp: &Path,
    digest: &str,
    generator_version: &str,
    lane: &str,
) -> Result<Vec<u8>, OrchestratorError> {
    let runner_temp = validate_runner_temp_path(runner_temp)?;
    validate_digest(digest).map_err(internal_contract)?;
    let lane = SeedLane::for_task_kind(lane).ok_or_else(|| internal("unknown_seed_lane"))?;
    let manifest = read_manifest(root, generator_version)?;
    let record = unique_record_by_execution_digest(&manifest, digest)?;
    record.validate().map_err(internal_contract)?;
    let kind = record.task_id.rsplit('/').nth(1).unwrap_or_default();
    if SeedLane::for_task_kind(kind) != Some(lane) {
        return Err(internal("seed_record_lane_mismatch"));
    }
    let argv = with_machine_output(lane, &record.argv)?;
    let mut fields = Vec::with_capacity(record.env.len() + argv.len());
    for (key, value) in &record.env {
        if !valid_env_key(key) {
            return Err(internal("seed_record_bad_env_key"));
        }
        fields.push(format!(
            "{key}={}",
            resolve_runner_temp_expression(value, runner_temp)?
        ));
    }
    for value in argv {
        fields.push(resolve_runner_temp_expression(&value, runner_temp)?);
    }
    let mut bytes = Vec::new();
    for field in fields {
        if field.is_empty() || field.contains('\0') {
            return Err(internal("seed_frame_bad_field"));
        }
        bytes.extend_from_slice(field.as_bytes());
        bytes.push(0);
    }
    if bytes.len() > MAX_TASK_EXECUTION_FRAME_BYTES {
        return Err(internal("seed_frame_too_large"));
    }
    Ok(bytes)
}

/// Insert the lane's machine-output flags right after the Cargo subcommand.
fn with_machine_output(lane: SeedLane, argv: &[String]) -> Result<Vec<String>, OrchestratorError> {
    let separator = argv
        .iter()
        .position(|arg| arg == "--")
        .ok_or_else(|| internal("seed_record_without_mise_separator"))?;
    let program = argv
        .get(separator + 1)
        .ok_or_else(|| internal("seed_record_without_program"))?;
    if program != "mbx" && program != "cargo" {
        return Err(internal("seed_record_not_cargo"));
    }
    let mut at = separator + 2;
    if argv.get(at).is_some_and(|arg| arg.starts_with('+')) {
        at += 1;
    }
    let subcommand = argv
        .get(at)
        .map(String::as_str)
        .ok_or_else(|| internal("seed_record_without_subcommand"))?;
    let (insert_at, flags): (usize, &[&str]) = match (lane, subcommand) {
        (SeedLane::Build, "nextest") if argv.get(at + 1).map(String::as_str) == Some("list") => {
            (at + 2, &["--cargo-message-format", JSON_DIAGNOSTICS])
        }
        (SeedLane::Build, "test" | "build") => (at + 1, &["--message-format", JSON_DIAGNOSTICS]),
        (SeedLane::Clippy, "clippy") | (SeedLane::Doc, "doc") => (
            at + 1,
            &["--message-format", JSON_DIAGNOSTICS, "--keep-going"],
        ),
        _ => return Err(internal("seed_record_subcommand_mismatch")),
    };
    let mut out = argv.to_vec();
    out.splice(
        insert_at..insert_at,
        flags.iter().map(|flag| (*flag).to_owned()),
    );
    Ok(out)
}

fn valid_env_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !key.starts_with("VELNOR_")
        && !velnor_actions_workflow_renderer::toolchain_env::is_denied_credential_key(key)
}

#[cfg(test)]
#[path = "seed_task_tests.rs"]
mod tests;
