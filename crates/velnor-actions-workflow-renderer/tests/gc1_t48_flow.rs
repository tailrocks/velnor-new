// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn run_flow(
    fixture: &Fixture,
    record_path: &str,
    archive_path: &str,
    identity_path: &str,
) -> Result<(i32, String), Box<dyn Error>> {
    let output = Command::new("python3")
        .arg(&fixture.driver)
        .arg(fixture.root.path())
        .arg(record_path)
        .arg(archive_path)
        .arg(identity_path)
        .output()?;
    let code = output
        .status
        .code()
        .ok_or_else(|| fail("fixed validator left no exit code"))?;
    let mut combined = String::from_utf8(output.stdout)?;
    combined.push_str(&String::from_utf8(output.stderr)?);
    Ok((code, combined))
}

pub(crate) fn tamper_layer_data(archive: &[u8], parts: &ImageParts) -> Vec<u8> {
    let mut offset = 0;
    for blob in [&parts.layout, &parts.index, &parts.manifest, &parts.config] {
        offset += 512 + blob.len().div_ceil(512) * 512;
    }
    let mut out = archive.to_vec();
    out[offset + 1025] ^= 1;
    out
}

pub(crate) fn check_fixed_body_flow(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let record_path = write_bytes(
        fixture.root.path(),
        "candidate.json",
        record_json(&parts, &archive).as_bytes(),
    )?;
    let archive_path = write_bytes(fixture.root.path(), "candidate.oci.tar", &archive)?;
    let identity_path = write_bytes(
        fixture.root.path(),
        "identity.json",
        identity_envelope().as_bytes(),
    )?;
    let (code, output) = run_flow(fixture, &record_path, &archive_path, &identity_path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed body rejected its valid candidate:{output}"
        )));
    }
    require(&output, "FLOW-OK", "data-only flow")?;
    let manifest_hex = sha256_hex(&parts.manifest);
    require(
        &output,
        &format!("digest_reference=ghcr.io/tailrocks/velnor-job@sha256:{manifest_hex}"),
        "evidence reference",
    )?;
    let evidence = std::fs::read_to_string(fixture.root.path().join("evidence.json"))?;
    require(
        &evidence,
        "\"schema\":\"velnor.job-image-publication.v1\"",
        "evidence shape",
    )?;
    require(&evidence, "\"status\":\"PASS\"", "evidence shape")?;
    require(&evidence, SOURCE_SHA, "evidence identity")?;
    Ok(())
}

pub(crate) fn check_flow_rejects_tampered_archive(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let record_path = write_bytes(
        fixture.root.path(),
        "candidate-tampered.json",
        record_json(&parts, &archive).as_bytes(),
    )?;
    let tampered = tamper_layer_data(&archive, &parts);
    let archive_path = write_bytes(fixture.root.path(), "tampered.oci.tar", &tampered)?;
    let identity_path = write_bytes(
        fixture.root.path(),
        "identity-tampered.json",
        identity_envelope().as_bytes(),
    )?;
    let (code, _) = run_flow(fixture, &record_path, &archive_path, &identity_path)?;
    if code == 0 {
        return Err(fail("fixed body accepted its tampered archive"));
    }
    Ok(())
}

pub(crate) fn check_flow_rejects_tampered_record(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let record = record_json(&parts, &archive).replacen(
        "velnor.job-image-candidate.v1",
        "other.schema.v1",
        1,
    );
    let record_path = write_bytes(fixture.root.path(), "candidate-bad.json", record.as_bytes())?;
    let archive_path = write_bytes(fixture.root.path(), "sized.oci.tar", &archive)?;
    let identity_path = write_bytes(
        fixture.root.path(),
        "identity-bad.json",
        identity_envelope().as_bytes(),
    )?;
    let (code, _) = run_flow(fixture, &record_path, &archive_path, &identity_path)?;
    if code == 0 {
        return Err(fail("fixed body accepted its tampered record"));
    }
    Ok(())
}

pub(crate) fn check_flow_rejects_mismatched_source(
    fixture: &Fixture,
) -> Result<(), Box<dyn Error>> {
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let record_path = write_bytes(
        fixture.root.path(),
        "candidate-identity.json",
        record_json(&parts, &archive).as_bytes(),
    )?;
    let archive_path = write_bytes(fixture.root.path(), "sized.oci.tar", &archive)?;
    let identity_path = write_bytes(
        fixture.root.path(),
        "identity-mismatch.json",
        identity_envelope()
            .replace(SOURCE_SHA, &"b".repeat(40))
            .as_bytes(),
    )?;
    let (code, _) = run_flow(fixture, &record_path, &archive_path, &identity_path)?;
    if code == 0 {
        return Err(fail(
            "fixed body relabeled unchanged bytes with another source SHA",
        ));
    }
    Ok(())
}

pub(crate) fn check_flow_rejects_missing_fragment(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    let record_path = write_bytes(
        fixture.root.path(),
        "candidate-fragment.json",
        record_json(&parts, &archive).as_bytes(),
    )?;
    let archive_path = write_bytes(fixture.root.path(), "fragment.oci.tar", &archive)?;
    let identity_path = write_bytes(
        fixture.root.path(),
        "identity-fragment.json",
        identity_envelope().as_bytes(),
    )?;
    std::fs::remove_file(fixture.root.path().join(RECORD_FILE))?;
    let (code, _) = run_flow(fixture, &record_path, &archive_path, &identity_path)?;
    if code == 0 {
        return Err(fail("fixed body survived its missing fragment"));
    }
    Ok(())
}

#[test]
fn gc1_t48_preserves_contract() -> Result<(), Box<dyn Error>> {
    let text = candidate_text()?;
    check_producer_job_kept(&text)?;
    check_attest_data_only(&text)?;
    check_publish_data_only(&text)?;
    check_shared_counts(&text)?;
    let again = candidate_text()?;
    if text != again {
        return Err(fail("authority render drifted between renders"));
    }
    let fixture = fixture()?;
    check_fixed_body_flow(&fixture)?;
    Ok(())
}

#[test]
fn gc1_t48_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let mut bare = request()?;
    bare.workflows = BTreeSet::new();
    let files = render_schema2_workflows(&bare)?;
    if files.iter().any(|file| file.path == CANDIDATE_PATH) {
        return Err(fail("render kept its candidate without image release"));
    }
    let mut unpinned = request()?;
    unpinned.product_release = None;
    match render_schema2_workflows(&unpinned) {
        Ok(_) => return Err(fail("render kept its candidate without native pins")),
        Err(error) => require(
            &error.to_string(),
            "product_release_pins_missing",
            "pins rejection",
        )?,
    }
    let fixture = fixture()?;
    check_flow_rejects_tampered_archive(&fixture)?;
    check_flow_rejects_tampered_record(&fixture)?;
    check_flow_rejects_mismatched_source(&fixture)?;
    check_flow_rejects_missing_fragment(&fixture)?;
    Ok(())
}
