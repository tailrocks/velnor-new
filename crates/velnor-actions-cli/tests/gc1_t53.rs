// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! PUBLISH-path binding regressions (threads 4389351082, 4389351092).
//!
//! Publication must verify both bindings before saving: the computed
//! inventory digest must equal `admission.inventory_digest` (a stale
//! admission carries obsolete measurements) and
//! `admission.generation.cache_key()` must equal `expected.key` (a
//! cross-generation admission authorizes a different candidate). Entry
//! lookup must match the full (key, ref, version) tuple, not the key
//! alone, so a same-key sibling under another ref can never be selected.

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
    env::temp_dir().join(format!("velnor-cli-t53-{}-{id}", std::process::id()))
}

fn cleanup(root: &PathBuf) {
    drop(fs::remove_dir_all(root));
}

fn run_cli(op: &str, runner_temp: &PathBuf, request: &PathBuf) -> Result<Output, Box<dyn Error>> {
    Command::new(binary())
        .env("VELNOR_INTERNAL_OP", op)
        .env("VELNOR_REQUEST_FILE", request)
        .env("RUNNER_TEMP", runner_temp)
        .output()
        .map_err(|error| fail(&format!("run cli binary: {error}")))
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

fn stdout_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
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

fn admission_record(
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

fn entry_full(cache_id: u64, key: &str, ref_name: &str, version: &str, size: u64) -> Value {
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

fn publish_body(admission: &Value, inventory: &Value, expected: &Value) -> Value {
    json!({"schema": 1, "operation": "publish", "admission": admission, "inventory": inventory, "expected": expected})
}

fn recover_body(inventory: &Value, expected: &Value) -> Value {
    json!({"schema": 1, "operation": "recover", "inventory": inventory, "expected": expected})
}

fn expected_full(key: &str, ref_name: &str, version: &str) -> Value {
    json!({"key": key, "ref": ref_name, "version": version})
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

fn payload_path(root: &Path) -> PathBuf {
    root.join("velnor-gc1/transport/state/payload.tar.gz")
}

fn write_payload(root: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let path = payload_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| fail(&format!("create payload dir: {error}")))?;
    }
    fs::write(&path, bytes).map_err(|error| fail(&format!("write payload: {error}")))?;
    Ok(())
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

#[test]
fn gc1_t53_publish_denies_stale_inventory_binding() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_full(&key, "refs/heads/main", "v1");
    let staging = scratch_root().join("staging-stale");
    let (root, _) = write_request(
        "cache-publish.json",
        &json!({"schema": 1, "operation": "publish"}),
    )?;
    let bytes = make_tarball(&root, &staging, &["state/payload"])?;
    drop(fs::remove_dir_all(&staging));
    let inventory = inventory_with(&[entry_full(
        7,
        &key,
        "refs/heads/main",
        "v1",
        bytes.len() as u64,
    )]);
    // A stale admission: decided over an older inventory, so its digest no
    // longer matches the current one.
    let stale = admission_record("admitted", &[], 0, &digest(9));
    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&stale, &inventory, &expected),
    )?;
    write_payload(&root, &bytes)?;
    let output = run_cli("cache-publish-v1", &root, &path)?;
    rejected(&output, "stale inventory binding")?;
    if stdout_text(&output).contains("state snapshot published") {
        return Err(fail("stale admission must never report published"));
    }
    cleanup(&root);
    Ok(())
}

#[test]
fn gc1_t53_publish_denies_cross_generation_binding() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_full(&key, "refs/heads/main", "v1");
    let staging = scratch_root().join("staging-generation");
    let (root, _) = write_request(
        "cache-publish.json",
        &json!({"schema": 1, "operation": "publish"}),
    )?;
    let bytes = make_tarball(&root, &staging, &["state/payload"])?;
    drop(fs::remove_dir_all(&staging));
    let inventory = inventory_with(&[entry_full(
        7,
        &key,
        "refs/heads/main",
        "v1",
        bytes.len() as u64,
    )]);
    // Cross-generation admission: digest matches, but the generation key is
    // repair 7 while the request expects repair 0.
    let cross = admission_record("admitted", &[], 7, &bound_digest(&inventory)?);
    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&cross, &inventory, &expected),
    )?;
    write_payload(&root, &bytes)?;
    let output = run_cli("cache-publish-v1", &root, &path)?;
    rejected(&output, "cross-generation binding")?;
    if stdout_text(&output).contains("state snapshot published") {
        return Err(fail(
            "cross-generation admission must never report published",
        ));
    }
    cleanup(&root);
    Ok(())
}

#[test]
fn gc1_t53_publish_accepts_bound_admission() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_full(&key, "refs/heads/main", "v1");
    let staging = scratch_root().join("staging-bound");
    let (root, _) = write_request(
        "cache-publish.json",
        &json!({"schema": 1, "operation": "publish"}),
    )?;
    let bytes = make_tarball(&root, &staging, &["state/payload"])?;
    drop(fs::remove_dir_all(&staging));
    let inventory = inventory_with(&[entry_full(
        7,
        &key,
        "refs/heads/main",
        "v1",
        bytes.len() as u64,
    )]);
    let bound = admission_record("admitted", &[], 0, &bound_digest(&inventory)?);
    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&bound, &inventory, &expected),
    )?;
    write_payload(&root, &bytes)?;
    let output = run_cli("cache-publish-v1", &root, &path)?;
    succeeded(&output, "bound admission")?;
    if !stdout_text(&output).contains("state snapshot published") {
        return Err(fail("bound admission must report published"));
    }
    cleanup(&root);
    Ok(())
}

#[test]
fn gc1_t53_publish_selects_full_tuple() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_full(&key, "refs/heads/feature", "v1");
    let staging = scratch_root().join("staging-tuple");
    let (root, _) = write_request(
        "cache-publish.json",
        &json!({"schema": 1, "operation": "publish"}),
    )?;
    let bytes = make_tarball(&root, &staging, &["state/payload"])?;
    drop(fs::remove_dir_all(&staging));
    let size = bytes.len() as u64;
    // Same logical key under two refs; the unrelated tuple sorts first by
    // cache id. Key-only lookup would select it and fail readback even
    // though the correct entry is present.
    let inventory = inventory_with(&[
        entry_full(7, &key, "refs/heads/main", "v1", size),
        entry_full(9, &key, "refs/heads/feature", "v1", size),
    ]);
    let bound = admission_record("admitted", &[], 0, &bound_digest(&inventory)?);
    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&bound, &inventory, &expected),
    )?;
    write_payload(&root, &bytes)?;
    let output = run_cli("cache-publish-v1", &root, &path)?;
    succeeded(&output, "full tuple selection")?;
    if !stdout_text(&output).contains("state snapshot published") {
        return Err(fail("full tuple selection must report published"));
    }
    cleanup(&root);
    Ok(())
}

#[test]
fn gc1_t53_recover_selects_full_tuple() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_full(&key, "refs/heads/feature", "v1");
    let staging = scratch_root().join("staging-recover-tuple");
    let build = scratch_root();
    fs::create_dir_all(&build).map_err(|error| fail(&format!("create build dir: {error}")))?;
    let bytes = make_tarball(&build, &staging, &["state/payload"])?;
    drop(fs::remove_dir_all(&build));
    drop(fs::remove_dir_all(&staging));
    let size = bytes.len() as u64;
    let inventory = inventory_with(&[
        entry_full(7, &key, "refs/heads/main", "v1", size),
        entry_full(9, &key, "refs/heads/feature", "v1", size),
    ]);
    let (root, path) = write_request("cache-recover.json", &recover_body(&inventory, &expected))?;
    write_payload(&root, &bytes)?;
    let output = run_cli("cache-recover-v1", &root, &path)?;
    succeeded(&output, "recover full tuple selection")?;
    if !stdout_text(&output).contains("state snapshot recovered") {
        return Err(fail("recover full tuple selection must report recovery"));
    }
    if !root.join("velnor-gc1/state/payload").is_file() {
        return Err(fail(
            "recover full tuple selection must restore the state root",
        ));
    }
    cleanup(&root);
    Ok(())
}
