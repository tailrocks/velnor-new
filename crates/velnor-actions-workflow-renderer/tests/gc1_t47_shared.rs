// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn identity_envelope() -> String {
    format!(
        "{{\"repository\":\"tailrocks/velnor-new\",\"repository_id\":1390620900,\"event\":\"workflow_dispatch\",\"workflow_ref\":\"{WORKFLOW_REF}\",\"ref\":\"refs/heads/main\",\"authority_commit\":\"{SOURCE_SHA}\",\"run_id\":7,\"run_attempt\":2,\"source\":\"{SOURCE_SHA}\",\"artifact_id\":9}}",
    )
}

pub(crate) fn record_source_envelope() -> String {
    format!(
        "{{\"commit\":\"{SOURCE_SHA}\",\"ref\":\"refs/heads/main\",\"inputs\":{{\".velnor/runner.lock\":\"{lock}\"}}}}",
        lock = "b".repeat(64),
    )
}

pub(crate) fn source_inputs_envelope() -> String {
    format!(
        "{{\"record_source\":{record},\"authenticated\":{record}}}",
        record = record_source_envelope(),
    )
}

pub(crate) fn build_lock_envelope() -> String {
    "{\"builder\":\"github.com/docker/buildx v0.25.0\",\"buildx_version\":\"0.25.0\"}".to_owned()
}

pub(crate) fn replace_once(
    haystack: &str,
    needle: &str,
    replacement: &str,
) -> Result<String, Box<dyn Error>> {
    if !haystack.contains(needle) {
        return Err(fail(&format!("fixture lost its mutation point:{needle}")));
    }
    Ok(haystack.replacen(needle, replacement, 1))
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

pub(crate) fn json_string(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let code = c as u32;
                out.push_str("\\u00");
                out.push(HEX[(code >> 4) as usize] as char);
                out.push(HEX[(code & 0x0F) as usize] as char);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn check_shared_body_attests(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    let record_path = write_bytes(
        fixture.root.path(),
        "candidate.json",
        record_json(parts, archive).as_bytes(),
    )?;
    let (code, output) = run_mode(fixture, "record_ok", &record_path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed body rejected its valid record:{output}"
        )));
    }
    require(&output, "RECORD-OK", "record acceptance")?;
    let archive_path = write_bytes(fixture.root.path(), "candidate.oci.tar", archive)?;
    let (code, output) = run_mode(fixture, "archive_ok", &archive_path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed body rejected its valid archive:{output}"
        )));
    }
    require(&output, "ARCHIVE-OK", "archive acceptance")?;
    require(
        &output,
        &format!(
            "platform_manifest_digest=sha256:{}",
            sha256_hex(&parts.manifest)
        ),
        "archive acceptance",
    )?;
    require(
        &output,
        &format!("layer_digests=sha256:{}", sha256_hex(&parts.layer)),
        "archive acceptance",
    )?;
    let (code, output) = run_mode(fixture, "tar_member", "blobs/sha256/abc")?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed body rejected its safe member:{output}"
        )));
    }
    require(&output, "TAR-MEMBER-OK", "tar acceptance")?;
    let output = expect_ok(
        fixture,
        "identity",
        &identity_envelope(),
        "identity-ok",
        "IDENTITY-OK",
    )?;
    require(&output, SOURCE_SHA, "identity acceptance")?;
    let output = expect_ok(
        fixture,
        "source_inputs",
        &source_inputs_envelope(),
        "source-inputs-ok",
        "SOURCE-INPUTS-OK",
    )?;
    require(&output, SOURCE_SHA, "source inputs acceptance")?;
    expect_ok(
        fixture,
        "build_lock",
        &build_lock_envelope(),
        "build-lock-ok",
        "BUILD-LOCK-OK",
    )?;
    Ok(())
}

pub(crate) fn check_shared_body_publishes(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<String, Box<dyn Error>> {
    let archive_path = write_bytes(fixture.root.path(), "blobs.oci.tar", archive)?;
    let archive_json = json_string(&archive_path);
    let manifest_hex = sha256_hex(&parts.manifest);
    let config_hex = sha256_hex(&parts.config);
    let layer_hex = sha256_hex(&parts.layer);
    let envelope = format!(
        "{{\"manifest\":{}}}",
        String::from_utf8(parts.manifest.clone())?,
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
        &format!("sha256:{layer_hex}"),
        "descriptors acceptance",
    )?;
    let manifest_len = parts.manifest.len();
    let envelope = format!(
        "{{\"archive\":{archive_json},\"digest\":\"sha256:{manifest_hex}\",\"size\":{manifest_len}}}"
    );
    let output = expect_ok(fixture, "manifest", &envelope, "manifest-ok", "MANIFEST-OK")?;
    require(
        &output,
        &format!("size={manifest_len}"),
        "manifest acceptance",
    )?;
    let layer_len = parts.layer.len();
    let envelope = format!(
        "{{\"archive\":{archive_json},\"digest\":\"sha256:{layer_hex}\",\"size\":{layer_len}}}"
    );
    let output = expect_ok(fixture, "extract", &envelope, "extract-ok", "EXTRACT-OK")?;
    require(&output, &format!("sha={layer_hex}"), "extract acceptance")?;
    let digest = format!("sha256:{}", sha256_hex(b"registry-head-bytes"));
    let envelope = format!(
        "{{\"path\":\"/v2/tailrocks/velnor-job/blobs/{digest}\",\"digest\":\"{digest}\",\"status\":200,\"location\":\"\"}}",
    );
    expect_ok(
        fixture,
        "redirect",
        &envelope,
        "redirect-direct",
        "REDIRECT-NONE",
    )?;
    check_shared_body_evidence(fixture, parts, archive, &manifest_hex)
}

pub(crate) fn check_shared_body_evidence(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
    manifest_hex: &str,
) -> Result<String, Box<dyn Error>> {
    let oci = format!(
        "{{\"archive_index_digest\":\"{index}\",\"platform_manifest_digest\":\"sha256:{manifest_hex}\",\"platform_manifest_size_bytes\":{manifest_len},\"image_configuration_digest\":\"sha256:{config_hex}\",\"image_configuration_size_bytes\":{config_len},\"layer_digests\":[\"sha256:{layer_hex}\"]}}",
        index = sha256_hex(&parts.index),
        manifest_len = parts.manifest.len(),
        config_len = parts.config.len(),
        config_hex = sha256_hex(&parts.config),
        layer_hex = sha256_hex(&parts.layer),
    );
    let envelope = format!(
        "{{\"identity\":{identity},\"ci\":{{\"run_id\":11,\"run_attempt\":1}},\"record_archive\":{{\"sha256\":\"{archive_sha}\",\"size_bytes\":{archive_len}}},\"oci\":{oci},\"manifest_hex\":\"{manifest_hex_bytes}\",\"record_sha256\":\"{record_sha}\"}}",
        identity = identity_envelope(),
        archive_sha = sha256_hex(archive),
        archive_len = archive.len(),
        manifest_hex_bytes = hex_encode(&parts.manifest),
        record_sha = sha256_hex(b"authority-record"),
    );
    let output = expect_ok(fixture, "evidence", &envelope, "evidence-ok", "EVIDENCE-OK")?;
    require(&output, "size=", "evidence acceptance")?;
    let evidence_path = fixture.root.path().join("evidence.json");
    let evidence = std::fs::read_to_string(&evidence_path)?;
    require(
        &evidence,
        "\"schema\":\"velnor.job-image-publication.v1\"",
        "evidence shape",
    )?;
    require(&evidence, "\"status\":\"PASS\"", "evidence shape")?;
    require(
        &evidence,
        &format!("\"digest_reference\":\"ghcr.io/tailrocks/velnor-job@sha256:{manifest_hex}\""),
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

pub(crate) fn tamper_layer_data(archive: &[u8], parts: &ImageParts) -> Vec<u8> {
    let mut offset = 0;
    for blob in [&parts.layout, &parts.index, &parts.manifest, &parts.config] {
        offset += 512 + blob.len().div_ceil(512) * 512;
    }
    let mut out = archive.to_vec();
    out[offset + 1025] ^= 1;
    out
}
