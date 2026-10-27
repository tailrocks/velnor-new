// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Shared PUBLISH-path fixtures for the `gc1_t53` binding regressions.

use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use velnor_actions_orchestrator::{
    HARD_CEILING_BYTES, MAX_GENERATION_SLOTS, OPERATING_CEILING_BYTES,
};

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

pub(crate) fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"))
}

pub(crate) fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

pub(crate) fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

pub(crate) fn generation_key(repair: u64) -> String {
    format!(
        "velnor-gc1-state-{}-{}-{}-r{repair}",
        digest(3),
        digest(4),
        digest(5)
    )
}

pub(crate) fn scratch_root() -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::SeqCst);
    env::temp_dir().join(format!("velnor-cli-t53-{}-{id}", std::process::id()))
}

pub(crate) fn cleanup(root: &PathBuf) {
    drop(fs::remove_dir_all(root));
}

pub(crate) fn run_cli(
    op: &str,
    runner_temp: &PathBuf,
    request: &PathBuf,
) -> Result<Output, Box<dyn Error>> {
    Command::new(binary())
        .env("VELNOR_INTERNAL_OP", op)
        .env("VELNOR_REQUEST_FILE", request)
        .env("RUNNER_TEMP", runner_temp)
        .output()
        .map_err(|error| fail(&format!("run cli binary: {error}")))
}

pub(crate) fn succeeded(output: &Output, what: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        Ok(())
    } else {
        Err(fail(&format!(
            "{what} must succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

pub(crate) fn rejected(output: &Output, what: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        Err(fail(&format!("{what} must fail")))
    } else {
        Ok(())
    }
}

pub(crate) fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Compute the canonical inventory digest for the exact inventory JSON
/// carried by the request, so the admission under test is bound to it.
pub(crate) fn bound_digest(inventory: &Value) -> Result<String, Box<dyn Error>> {
    let parsed: velnor_actions_orchestrator::InventoryV1 =
        serde_json::from_value(inventory.clone())
            .map_err(|error| fail(&format!("encode inventory: {error}")))?;
    velnor_actions_orchestrator::inventory_digest(&parsed)
        .map(|digest| digest.as_str().to_owned())
        .map_err(|error| fail(&format!("digest inventory: {error}")))
}

pub(crate) fn admission_record(
    decision: &str,
    blocked_reasons: &[&str],
    repair: u64,
    inventory_digest: &str,
) -> Value {
    json!({
        "schema": 1,
        "job": {
            "schema": 1,
            "repository_id": 1,
            "repository": "tailrocks/velnor-new",
            "run_id": 7,
            "run_attempt": 1,
            "job_id": 11,
            "job_key": "writer",
            "event": "push",
            "ref": "refs/heads/main",
            "workflow_sha": "a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5",
            "source_sha": "5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a",
            "plan_digest": digest(6),
        },
        "generation": {
            "family": "state",
            "group_digest": digest(3),
            "compatibility_digest": digest(4),
            "coverage_digest": digest(5),
            "repair_revision": repair,
        },
        "active_intent_digest": digest(1),
        "inventory_digest": inventory_digest,
        "hard_ceiling_bytes": HARD_CEILING_BYTES,
        "operating_ceiling_bytes": OPERATING_CEILING_BYTES,
        "inventory_bytes": {"value": 100, "unknown_reason": null},
        "pending_owned_bytes": {"value": 0, "unknown_reason": null},
        "foreign_inflight_bytes": {"value": 0, "unknown_reason": null},
        "candidate_inner_bytes": {"value": 200, "unknown_reason": null},
        "measured_outer_bytes": {"value": 200, "unknown_reason": null},
        "qualified_repack_allowance_bytes": 0,
        "outer_envelope_identity": "gc1-t53-tool",
        "candidate_upper_bound_bytes": {"value": 200, "unknown_reason": null},
        "occupied_generation_slots": 0,
        "maximum_generation_slots": MAX_GENERATION_SLOTS,
        "peak_local_disk_bound": {"value": 300, "unknown_reason": null},
        "available_local_disk": {"value": 400, "unknown_reason": null},
        "local_disk_reserve": 0,
        "decision": decision,
        "blocked_reasons": blocked_reasons,
    })
}

pub(crate) fn entry_full(
    cache_id: u64,
    key: &str,
    ref_name: &str,
    version: &str,
    size: u64,
) -> Value {
    json!({
        "repository_id": 1,
        "cache_id": cache_id,
        "ref": ref_name,
        "key": key,
        "version": version,
        "size_in_bytes": size,
        "created_at": "2026-10-26T00:00:00Z",
        "last_accessed_at": "2026-10-26T00:00:00Z",
    })
}

pub(crate) fn inventory_with(entries: &[Value]) -> Value {
    let total: u64 = entries
        .iter()
        .map(|entry| entry["size_in_bytes"].as_u64().unwrap_or(0))
        .sum();
    json!({
        "schema": 1,
        "repository_id": 1,
        "captured_at": "2026-10-26T00:00:00Z",
        "entries": entries,
        "complete": true,
        "pages_observed": 1,
        "unknown_reason": null,
        "sum_stored_bytes": {"value": total, "unknown_reason": null},
    })
}

pub(crate) fn publish_body(admission: &Value, inventory: &Value, expected: &Value) -> Value {
    json!({"schema": 1, "operation": "publish", "admission": admission, "inventory": inventory, "expected": expected})
}

pub(crate) fn recover_body(inventory: &Value, expected: &Value) -> Value {
    json!({"schema": 1, "operation": "recover", "inventory": inventory, "expected": expected})
}

pub(crate) fn expected_full(key: &str, ref_name: &str, version: &str) -> Value {
    json!({"key": key, "ref": ref_name, "version": version})
}

pub(crate) fn write_request(
    name: &str,
    body: &Value,
) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let root = scratch_root();
    let dir = root.join("velnor");
    fs::create_dir_all(&dir).map_err(|error| fail(&format!("create scratch dir: {error}")))?;
    let path = dir.join(name);
    let bytes =
        serde_json::to_vec(body).map_err(|error| fail(&format!("encode request: {error}")))?;
    fs::write(&path, bytes).map_err(|error| fail(&format!("write request: {error}")))?;
    Ok((root, path))
}

pub(crate) fn payload_path(root: &Path) -> PathBuf {
    root.join("velnor-gc1/transport/state/payload.tar.gz")
}

pub(crate) fn write_payload(root: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let path = payload_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| fail(&format!("create payload dir: {error}")))?;
    }
    fs::write(&path, bytes).map_err(|error| fail(&format!("write payload: {error}")))?;
    Ok(())
}

pub(crate) fn make_tarball(
    root: &Path,
    staging: &Path,
    members: &[&str],
) -> Result<Vec<u8>, Box<dyn Error>> {
    drop(fs::remove_dir_all(staging));
    fs::create_dir_all(staging).map_err(|error| fail(&format!("create staging: {error}")))?;
    for member in members {
        let path = staging.join(member);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| fail(&format!("create member dir: {error}")))?;
        }
        fs::write(&path, format!("fixture bytes for {member}"))
            .map_err(|error| fail(&format!("write member: {error}")))?;
    }
    let archive = root.join("fixture.tar.gz");
    let output = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(staging)
        .arg("state")
        .output()
        .map_err(|error| fail(&format!("run tar: {error}")))?;
    if !output.status.success() {
        return Err(fail("tar pack failed"));
    }
    fs::read(&archive).map_err(|error| fail(&format!("read archive: {error}")))
}
