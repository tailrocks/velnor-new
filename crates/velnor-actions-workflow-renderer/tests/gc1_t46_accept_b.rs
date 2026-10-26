// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_accepts_retrieval(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let body = b"registry-get-bytes";
    let digest = format!("sha256:{}", sha256_hex(body));
    let body_hex = hex_encode(body);
    let envelope = format!(
        concat!(
            "{{\"status\":200,\"headers\":{{\"docker-content-digest\":\"{digest}\",",
            "\"content-length\":\"{len}\"}},\"body_hex\":\"{body}\",",
            "\"digest\":\"{digest}\",\"size\":{len},\"expected_hex\":\"{body}\"}}"
        ),
        digest = digest,
        len = body.len(),
        body = body_hex,
    );
    let output = expect_ok(fixture, "get", &envelope, "get-ok", "GET-OK")?;
    require(&output, &format!("size={}", body.len()), "get acceptance")?;
    let blob_path = format!("/v2/tailrocks/velnor-job/blobs/{digest}");
    let envelope = format!(
        concat!(
            "{{\"path\":\"{path}\",\"digest\":\"{digest}\",\"status\":302,",
            "\"location\":\"https://pkg-containers.githubusercontent.com/sha256/abcd/blobs/{digest}\"}}"
        ),
        path = blob_path,
        digest = digest,
    );
    let output = expect_ok(fixture, "redirect", &envelope, "redirect-ok", "REDIRECT-OK")?;
    require(
        &output,
        &format!("/sha256/abcd/blobs/{digest}"),
        "redirect acceptance",
    )?;
    let envelope = format!(
        concat!(
            "{{\"path\":\"{path}\",\"digest\":\"{digest}\",\"status\":200,",
            "\"location\":\"\"}}"
        ),
        path = blob_path,
        digest = digest,
    );
    expect_ok(
        fixture,
        "redirect",
        &envelope,
        "redirect-direct",
        "REDIRECT-NONE",
    )?;
    Ok(())
}

pub(crate) fn check_accepts_publication(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let (_config, config_hex, data, data_hex) = blob_case();
    let (manifest, manifest_hex) = manifest_case();
    let (archive, _, _, _, _) = archive_case();
    let archive_path = write_archive(fixture, "blobs.oci.tar", &archive)?;
    let archive_json = json_string(&archive_path);
    let manifest_digest = format!("sha256:{manifest_hex}");
    let envelope = format!(
        "{{\"manifest\":{manifest}}}",
        manifest = String::from_utf8(manifest.clone())?,
    );
    let output = expect_ok(
        fixture,
        "descriptors",
        &envelope,
        "descriptors-ok",
        "DESCRIPTORS-OK",
    )?;
    require(
        &output,
        &format!("sha256:{config_hex}"),
        "descriptors acceptance",
    )?;
    require(
        &output,
        &format!("sha256:{data_hex}"),
        "descriptors acceptance",
    )?;
    let envelope = format!(
        "{{\"archive\":{archive},\"digest\":\"{digest}\",\"size\":{size}}}",
        archive = archive_json,
        digest = manifest_digest,
        size = manifest.len(),
    );
    let output = expect_ok(fixture, "manifest", &envelope, "manifest-ok", "MANIFEST-OK")?;
    require(
        &output,
        &format!("size={}", manifest.len()),
        "manifest acceptance",
    )?;
    let data_digest = format!("sha256:{data_hex}");
    let envelope = format!(
        "{{\"archive\":{archive},\"digest\":\"{digest}\",\"size\":{size}}}",
        archive = archive_json,
        digest = data_digest,
        size = data.len(),
    );
    let output = expect_ok(fixture, "extract", &envelope, "extract-ok", "EXTRACT-OK")?;
    require(&output, &format!("sha={data_hex}"), "extract acceptance")?;
    require(
        &output,
        &format!("size={}", data.len()),
        "extract acceptance",
    )?;
    Ok(())
}

pub(crate) fn evidence_envelope(
    fixture: &Fixture,
    manifest: &[u8],
    manifest_hex: &str,
    archive: &[u8],
) -> Result<(String, String), Box<dyn Error>> {
    let (_config, config_hex, _data, data_hex) = blob_case();
    let oci = format!(
        concat!(
            "{{\"archive_index_digest\":\"{index}\",",
            "\"platform_manifest_digest\":\"sha256:{manifest_hex}\",",
            "\"platform_manifest_size_bytes\":{manifest_len},",
            "\"image_configuration_digest\":\"sha256:{config_hex}\",",
            "\"image_configuration_size_bytes\":{config_len},",
            "\"layer_digests\":[\"sha256:{data_hex}\"]}}"
        ),
        index = sha256_hex(b"evidence-index"),
        manifest_hex = manifest_hex,
        manifest_len = manifest.len(),
        config_hex = config_hex,
        config_len = blob_case().0.len(),
        data_hex = data_hex,
    );
    let envelope = format!(
        concat!(
            "{{\"identity\":{identity},\"ci\":{{\"run_id\":11,\"run_attempt\":1}},",
            "\"record_archive\":{{\"sha256\":\"{archive_sha}\",\"size_bytes\":{archive_len}}},",
            "\"oci\":{oci},\"manifest_hex\":\"{manifest_hex_bytes}\",",
            "\"record_sha256\":\"{record_sha}\"}}"
        ),
        identity = identity_envelope(),
        archive_sha = sha256_hex(archive),
        archive_len = archive.len(),
        oci = oci,
        manifest_hex_bytes = hex_encode(manifest),
        record_sha = sha256_hex(b"evidence-record"),
    );
    let output = expect_ok(fixture, "evidence", &envelope, "evidence-ok", "EVIDENCE-OK")?;
    require(&output, "size=", "evidence acceptance")?;
    let evidence_path = fixture.root.path().join("evidence.json");
    let evidence = std::fs::read_to_string(&evidence_path)?;
    Ok((evidence, manifest_hex.to_owned()))
}

pub(crate) fn check_accepts_evidence(fixture: &Fixture) -> Result<String, Box<dyn Error>> {
    let (manifest, manifest_hex) = manifest_case();
    let (archive, _, _, _, _) = archive_case();
    let (evidence, _) = evidence_envelope(fixture, &manifest, &manifest_hex, &archive)?;
    require(
        &evidence,
        "\"schema\":\"velnor.job-image-publication.v1\"",
        "evidence shape",
    )?;
    require(&evidence, "\"status\":\"PASS\"", "evidence shape")?;
    require(
        &evidence,
        "\"package\":\"ghcr.io/tailrocks/velnor-job\"",
        "evidence shape",
    )?;
    require(
        &evidence,
        &format!("\"digest_reference\":\"ghcr.io/tailrocks/velnor-job@sha256:{manifest_hex}\""),
        "evidence shape",
    )?;
    require(
        &evidence,
        "\"private_package_access_gate\":\"not_proven\"",
        "evidence shape",
    )?;
    require(&evidence, SOURCE_SHA, "evidence shape")?;
    let envelope = format!("{{\"evidence\":{evidence}}}");
    let output = expect_ok(
        fixture,
        "evidence_check",
        &envelope,
        "evidence-check-ok",
        "EVIDENCE-VALID",
    )?;
    require(
        &output,
        &format!("ghcr.io/tailrocks/velnor-job@sha256:{manifest_hex}"),
        "evidence validation acceptance",
    )?;
    Ok(evidence)
}

#[test]
fn gc1_t46_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_sources_are_fixed()?;
    let fixture = fixture()?;
    check_accepts_identity(&fixture)?;
    check_accepts_authorization(&fixture)?;
    check_accepts_registry_rules(&fixture)?;
    check_accepts_registry_auth(&fixture)?;
    check_accepts_retrieval(&fixture)?;
    check_accepts_publication(&fixture)?;
    check_accepts_evidence(&fixture)?;
    Ok(())
}
