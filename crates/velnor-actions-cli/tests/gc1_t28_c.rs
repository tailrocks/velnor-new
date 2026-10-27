// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::os::unix::fs::FileTypeExt;

#[test]
fn gc1_t28_recover_length_mismatch_falls_back_cold() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_body(&key);
    let staging = scratch_root().join("staging-mismatch");
    let build = scratch_root();
    fs::create_dir_all(&build).map_err(|error| fail(&format!("create build dir: {error}")))?;
    let bytes = make_tarball(&build, &staging, &["state/payload"])?;
    drop(fs::remove_dir_all(&build));
    drop(fs::remove_dir_all(&staging));
    // Inventory disagrees with the restored payload by five bytes.
    let mismatched = inventory_with(&[entry(7, 0, bytes.len() as u64 + 5)]);
    let (root, _) = write_request("cache-recover.json", &recover_body(&mismatched, &expected))?;
    write_payload(&root, &bytes)?;
    let request_path = root.join("velnor/cache-recover.json");
    let output = run_cli("cache-recover-v1", &root, &request_path)?;
    succeeded(&output, "length mismatch must fall back cold")?;
    if !stdout_text(&output).contains("cache_corrupt") {
        return Err(fail("length mismatch must report cache_corrupt"));
    }
    if payload_path(&root).exists() {
        return Err(fail("length mismatch must remove the payload"));
    }
    if root.join("velnor-gc1/state").exists() {
        return Err(fail("length mismatch must not touch the state root"));
    }
    cleanup(&root);
    Ok(())
}

#[test]
fn gc1_t28_recover_refuses_special_nodes() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_body(&key);
    let build = scratch_root();
    let staging = build.join("staging-special");
    fs::create_dir_all(staging.join("state"))
        .map_err(|error| fail(&format!("create staging: {error}")))?;
    fs::write(staging.join("state/ok"), "fixture state bytes")
        .map_err(|error| fail(&format!("write member: {error}")))?;
    let fifo = staging.join("state/pipe");
    let status = Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .map_err(|error| fail(&format!("run mkfifo: {error}")))?;
    if !status.success() {
        return Err(fail("mkfifo failed"));
    }
    if !fs::symlink_metadata(&fifo)
        .map(|meta| meta.file_type().is_fifo())
        .unwrap_or(false)
    {
        return Err(fail("fixture fifo was not created"));
    }
    let archive = build.join("special.tar.gz");
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
    let bytes = fs::read(&archive).map_err(|error| fail(&format!("read archive: {error}")))?;
    let special_inventory = inventory_with(&[entry(7, 0, bytes.len() as u64)]);
    let (root, path) = write_request(
        "cache-recover.json",
        &recover_body(&special_inventory, &expected),
    )?;
    write_payload(&root, &bytes)?;
    drop(fs::remove_dir_all(&build));
    let output = run_cli("cache-recover-v1", &root, &path)?;
    rejected(&output, "special node")?;
    if stdout_text(&output).contains("state snapshot recovered") {
        return Err(fail("special node must never report recovery"));
    }
    if root.join("velnor-gc1/state").exists() {
        return Err(fail("special node must not touch the state root"));
    }
    cleanup(&root);
    Ok(())
}
