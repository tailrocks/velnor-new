// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t28_preserves_contract() -> Result<(), Box<dyn Error>> {
    let admitted = admission_record("admitted", &[]);
    let key = generation_key(0);
    let expected = expected_body(&key);

    let staging = scratch_root().join("staging-first");
    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&admitted, &inventory_record(), &expected),
    )?;
    let first_bytes = make_tarball(&root, &staging, &["state/payload"])?;
    write_payload(&root, &first_bytes)?;
    drop(fs::remove_dir_all(&staging));
    let output = run_cli("cache-publish-v1", &root, &path)?;
    succeeded(&output, "publish")?;
    if !stdout_text(&output).contains("state snapshot published") {
        return Err(fail("publish must report after exact readback"));
    }
    cleanup(&root);

    let exact_inventory = inventory_with(&[entry(7, 0, first_bytes.len() as u64)]);
    let (root, path) = write_request(
        "cache-publish.json",
        &publish_body(&admitted, &exact_inventory, &expected),
    )?;
    write_payload(&root, &first_bytes)?;
    let output = run_cli("cache-publish-v1", &root, &path)?;
    succeeded(&output, "republish verifies against the observed entry")?;
    if !stdout_text(&output).contains("state snapshot published") {
        return Err(fail("republish must report after readback"));
    }
    cleanup(&root);

    let (root, path) = write_request(
        "cache-recover.json",
        &recover_body(&exact_inventory, &expected),
    )?;
    write_payload(&root, &first_bytes)?;
    let output = run_cli("cache-recover-v1", &root, &path)?;
    succeeded(&output, "recover")?;
    if !stdout_text(&output).contains("state snapshot recovered") {
        return Err(fail("recover must report after exact readback"));
    }
    if !root.join("velnor-gc1/state/payload").is_file() {
        return Err(fail("recover must restore the state root"));
    }
    cleanup(&root);

    let (root, path) = write_request(
        "cache-cleanup.json",
        &cleanup_body(&inventory_record(), &json!([7])),
    )?;
    let output = run_cli("cache-cleanup-v1", &root, &path)?;
    succeeded(&output, "cleanup")?;
    cleanup(&root);
    Ok(())
}

#[test]
fn gc1_t28_recover_refuses_hostile_archive() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_body(&key);
    let build = scratch_root();
    fs::create_dir_all(&build).map_err(|error| fail(&format!("create build dir: {error}")))?;
    let staging = scratch_root().join("staging-hostile");
    let hostile = make_hostile_tarball(&build, &staging)?;
    let hostile_inventory = inventory_with(&[entry(7, 0, hostile.len() as u64)]);
    let (root, path) = write_request(
        "cache-recover.json",
        &recover_body(&hostile_inventory, &expected),
    )?;
    write_payload(&root, &hostile)?;
    drop(fs::remove_dir_all(&build));
    let output = run_cli("cache-recover-v1", &root, &path)?;
    rejected(&output, "hostile archive")?;
    if stdout_text(&output).contains("state snapshot recovered") {
        return Err(fail("hostile archive must never report recovery"));
    }
    if root.join("velnor-gc1/state").exists() || root.join("velnor/state-stage").exists() {
        return Err(fail("hostile archive must not touch the state root"));
    }
    if !staging.join("tools/sentinel").is_file() {
        return Err(fail("hostile fixture source must stay intact"));
    }
    drop(fs::remove_dir_all(&staging));
    cleanup(&root);
    Ok(())
}

#[test]
fn gc1_t28_recover_cold_missing_stays_green() -> Result<(), Box<dyn Error>> {
    let key = generation_key(0);
    let expected = expected_body(&key);
    let (root, path) = write_request(
        "cache-recover.json",
        &recover_body(&inventory_record(), &expected),
    )?;
    let output = run_cli("cache-recover-v1", &root, &path)?;
    succeeded(&output, "cold missing")?;
    if !stdout_text(&output).contains("no_entry") {
        return Err(fail("cold missing must say so"));
    }
    cleanup(&root);
    Ok(())
}
