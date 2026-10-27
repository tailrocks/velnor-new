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

mod support;

use std::error::Error;
use std::fs;

use serde_json::json;

use support::*;
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
