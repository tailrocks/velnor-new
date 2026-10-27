// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_rejects_invalid_inputs(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    expect_rejection(
        fixture,
        "identity",
        &replace_once(
            &identity_envelope(),
            &format!("\"authority_commit\":\"{SOURCE_SHA}\""),
            "\"authority_commit\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"",
        )?,
        "identity-authority",
        "IDENTITY-REJECTED",
        Some("Workflow authority SHA is different from source SHA."),
    )?;
    let envelope = replace_once(
        &source_inputs_envelope(),
        &format!("\"commit\":\"{SOURCE_SHA}\""),
        "\"commit\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"",
    )?;
    expect_rejection(
        fixture,
        "source_inputs",
        &envelope,
        "source-inputs-tampered",
        "SOURCE-INPUTS-REJECTED",
        Some("Candidate source is not the authenticated source."),
    )?;
    let record_path = write_bytes(
        fixture.root.path(),
        "candidate-bad.json",
        record_json(parts, archive)
            .replacen("velnor.job-image-candidate.v1", "other.schema.v1", 1)
            .as_bytes(),
    )?;
    let (code, output) = run_mode(fixture, "record_bad", &record_path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed body left its record rejection:{output}"
        )));
    }
    require(&output, "RECORD-REJECTED", "record rejection")?;
    let tampered_archive = tamper_layer_data(archive, parts);
    let tampered_path = write_bytes(fixture.root.path(), "tampered.oci.tar", &tampered_archive)?;
    let (code, output) = run_mode(fixture, "archive_bad", &tampered_path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed body left its archive rejection:{output}"
        )));
    }
    require(&output, "ARCHIVE-REJECTED", "archive rejection")?;
    let (code, output) = run_mode(fixture, "tar_member", "/etc/passwd")?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed body left its member rejection:{output}"
        )));
    }
    require(&output, "TAR-MEMBER-REJECTED", "tar rejection")?;
    check_rejects_descriptors_shape(fixture)?;
    check_rejects_mismatched_source(fixture, parts, archive)?;
    let archive_path = write_bytes(fixture.root.path(), "sized.oci.tar", archive)?;
    let envelope = format!(
        "{{\"archive\":{},\"digest\":\"sha256:{}\",\"size\":{}}}",
        json_string(&archive_path),
        sha256_hex(&parts.manifest),
        parts.manifest.len() + 1,
    );
    expect_rejection(
        fixture,
        "manifest",
        &envelope,
        "manifest-size",
        "MANIFEST-REJECTED",
        Some("OCI platform manifest bytes changed after validation."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_descriptors_shape(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    expect_rejection(
        fixture,
        "descriptors",
        "{\"manifest\":{}}",
        "descriptors-shape",
        "DESCRIPTORS-REJECTED",
        Some("OCI manifest blob descriptors are incorrect."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_mismatched_source(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    let envelope = evidence_body_envelope(parts, archive, &"b".repeat(40));
    expect_rejection(
        fixture,
        "evidence",
        &envelope,
        "evidence-mismatched-source",
        "EVIDENCE-REJECTED",
        Some("Candidate source is not the authenticated source."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_missing_fragment(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    std::fs::remove_file(fixture.root.path().join(TAR_FILE))?;
    let (code, _) = run_mode(fixture, "tar_member", "blobs/sha256/abc")?;
    if code == 0 {
        return Err(fail("fixed body survived its missing fragment"));
    }
    Ok(())
}

#[test]
fn gc1_t47_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_body_composition()?;
    let text = candidate_text()?;
    check_modes_share_body(&text)?;
    let again = candidate_text()?;
    if text != again {
        return Err(fail("authority render drifted between renders"));
    }
    let fixture = fixture()?;
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    check_shared_body_attests(&fixture, &parts, &archive)?;
    check_shared_body_publishes(&fixture, &parts, &archive)?;
    Ok(())
}

#[test]
fn gc1_t47_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
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
    let parts = image_parts();
    let archive = archive_bytes(&parts);
    check_rejects_invalid_inputs(&fixture, &parts, &archive)?;
    check_rejects_missing_fragment(&fixture)?;
    Ok(())
}
