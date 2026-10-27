// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"))
}

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn digest(byte: u8) -> String {
    format!("b3-{}", format!("{byte:02x}").repeat(32))
}

fn generation_key(repair: u64) -> String {
    format!(
        "velnor-gc1-state-{}-{}-{}-r{repair}",
        digest(3),
        digest(4),
        digest(5)
    )
}

fn scratch_root() -> PathBuf {
    let id = NEXT_SCRATCH.fetch_add(1, Ordering::SeqCst);
    env::temp_dir().join(format!("velnor-cli-t28-{}-{id}", std::process::id()))
}

fn write_request(name: &str, body: &Value) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let root = scratch_root();
    let dir = root.join("velnor");
    fs::create_dir_all(&dir).map_err(|error| fail(&format!("create scratch dir: {error}")))?;
    let path = dir.join(name);
    let bytes =
        serde_json::to_vec(body).map_err(|error| fail(&format!("encode request: {error}")))?;
    fs::write(&path, bytes).map_err(|error| fail(&format!("write request: {error}")))?;
    Ok((root, path))
}

fn run_cli(op: &str, runner_temp: &PathBuf, request: &PathBuf) -> Result<Output, Box<dyn Error>> {
    Command::new(binary())
        .env("VELNOR_INTERNAL_OP", op)
        .env("VELNOR_REQUEST_FILE", request)
        .env("RUNNER_TEMP", runner_temp)
        .output()
        .map_err(|error| fail(&format!("run cli binary: {error}")))
}

fn cleanup(root: &PathBuf) {
    if fs::remove_dir_all(root).is_err() {
        // Best effort: scratch cleanup must not fail the test.
    }
}

fn succeeded(output: &Output, what: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        Ok(())
    } else {
        Err(fail(&format!(
            "{what} must succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

fn rejected(output: &Output, what: &str) -> Result<(), Box<dyn Error>> {
    if output.status.success() {
        Err(fail(&format!("{what} must fail")))
    } else {
        Ok(())
    }
}

/// Compute the canonical inventory digest for the exact inventory JSON
/// carried by the request, so the admission under test is bound to it.
fn bound_digest(inventory: &Value) -> Result<String, Box<dyn Error>> {
    let parsed: velnor_actions_orchestrator::InventoryV1 =
        serde_json::from_value(inventory.clone())
            .map_err(|error| fail(&format!("encode inventory: {error}")))?;
    velnor_actions_orchestrator::inventory_digest(&parsed)
        .map(|digest| digest.as_str().to_owned())
        .map_err(|error| fail(&format!("digest inventory: {error}")))
}

fn admission_record(decision: &str, blocked_reasons: &[&str], inventory_digest: &str) -> Value {
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
            "repair_revision": 0,
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
        "outer_envelope_identity": "gc1-t28-tool",
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

fn entry(cache_id: u64, repair: u64, size: u64) -> Value {
    json!({
        "repository_id": 1,
        "cache_id": cache_id,
        "ref": "refs/heads/main",
        "key": generation_key(repair),
        "version": "v1",
        "size_in_bytes": size,
        "created_at": "2026-10-26T00:00:00Z",
        "last_accessed_at": "2026-10-26T00:00:00Z",
    })
}

fn inventory_record() -> Value {
    json!({
        "schema": 1,
        "repository_id": 1,
        "captured_at": "2026-10-26T00:00:00Z",
        "entries": [entry(7, 0, 100), entry(9, 1, 50)],
        "complete": true,
        "pages_observed": 1,
        "unknown_reason": null,
        "sum_stored_bytes": {"value": 150, "unknown_reason": null},
    })
}

fn publish_body(admission: &Value, inventory: &Value, expected: &Value) -> Value {
    json!({"schema": 1, "operation": "publish", "admission": admission, "inventory": inventory, "expected": expected})
}

fn recover_body(inventory: &Value, expected: &Value) -> Value {
    json!({"schema": 1, "operation": "recover", "inventory": inventory, "expected": expected})
}

fn expected_body(key: &str) -> Value {
    json!({"key": key, "ref": "refs/heads/main", "version": "v1"})
}

fn inventory_with(entries: &[Value]) -> Value {
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

fn payload_path(root: &Path) -> PathBuf {
    root.join("velnor-gc1/transport/state/payload.tar.gz")
}

fn write_payload(root: &Path, bytes: &[u8]) -> Result<PathBuf, Box<dyn Error>> {
    let path = payload_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| fail(&format!("create payload dir: {error}")))?;
    }
    fs::write(&path, bytes).map_err(|error| fail(&format!("write payload: {error}")))?;
    Ok(path)
}

fn make_tarball(root: &Path, staging: &Path, members: &[&str]) -> Result<Vec<u8>, Box<dyn Error>> {
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

fn make_hostile_tarball(root: &Path, staging: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    drop(fs::remove_dir_all(staging));
    fs::create_dir_all(staging.join("state"))
        .map_err(|error| fail(&format!("create staging: {error}")))?;
    fs::create_dir_all(staging.join("tools"))
        .map_err(|error| fail(&format!("create staging: {error}")))?;
    fs::write(staging.join("state/ok"), "fixture state bytes")
        .map_err(|error| fail(&format!("write member: {error}")))?;
    fs::write(staging.join("tools/sentinel"), "must never land")
        .map_err(|error| fail(&format!("write sentinel: {error}")))?;
    let archive = root.join("hostile.tar.gz");
    let output = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(staging)
        .arg("state")
        .arg("tools")
        .output()
        .map_err(|error| fail(&format!("run tar: {error}")))?;
    if !output.status.success() {
        return Err(fail("tar pack failed"));
    }
    fs::read(&archive).map_err(|error| fail(&format!("read archive: {error}")))
}

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn cleanup_body(inventory: &Value, candidate_ids: &Value) -> Value {
    json!({"schema": 1, "operation": "cleanup", "inventory": inventory, "candidate_ids": candidate_ids})
}

#[path = "gc1_t28_a.rs"]
mod gc1_t28_a;
#[path = "gc1_t28_b.rs"]
mod gc1_t28_b;
#[path = "gc1_t28_c.rs"]
mod gc1_t28_c;
