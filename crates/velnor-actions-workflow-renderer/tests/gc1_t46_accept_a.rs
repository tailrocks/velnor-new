// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_accepts_identity(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let identity = identity_envelope();
    let output = expect_ok(fixture, "identity", &identity, "identity-ok", "IDENTITY-OK")?;
    require(&output, SOURCE_SHA, "identity acceptance")?;
    let path = fixture.root.path().join("identity.json");
    std::fs::write(&path, &identity)?;
    let envelope = format!(
        "{{\"path\":{}}}",
        json_string(
            path.to_str()
                .ok_or_else(|| fail("identity path left its string shape"))?
        )
    );
    expect_ok(
        fixture,
        "identity_file",
        &envelope,
        "identity-file-ok",
        "IDENTITY-OK",
    )?;
    Ok(())
}

pub(crate) fn check_accepts_authorization(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let output = expect_ok(
        fixture,
        "repository",
        &repository_envelope(),
        "repository-ok",
        "REPOSITORY-OK",
    )?;
    require(&output, SOURCE_SHA, "repository acceptance")?;
    expect_ok(
        fixture,
        "candidate_identity",
        &candidate_identity_envelope(),
        "candidate-identity-ok",
        "CANDIDATE-IDENTITY-OK",
    )?;
    let output = expect_ok(
        fixture,
        "artifact",
        &artifact_envelope(),
        "artifact-ok",
        "ARTIFACT-OK",
    )?;
    require(&output, "artifact_id=9", "artifact acceptance")?;
    let output = expect_ok(fixture, "ci", &ci_envelope(), "ci-ok", "CI-OK")?;
    require(&output, "run_id=11 run_attempt=1", "ci acceptance")?;
    let output = expect_ok(
        fixture,
        "attestation",
        &attestation_envelope(),
        "attestation-ok",
        "ATTESTATION-OK",
    )?;
    require(&output, "records=1", "attestation acceptance")?;
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

pub(crate) fn check_accepts_registry_rules(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let output = expect_ok(
        fixture,
        "registry_url",
        "{\"path\":\"manifests/sha256:abc\"}",
        "registry-url-ok",
        "REGISTRY-URL-OK",
    )?;
    require(
        &output,
        "https://ghcr.io/v2/tailrocks/velnor-job/manifests/sha256:abc",
        "registry url acceptance",
    )?;
    let digest = format!("sha256:{}", "c".repeat(64));
    let envelope = format!(
        "{{\"location\":\"https://pkg-containers.githubusercontent.com/sha256/abcd/blobs/{digest}\",\"digest\":\"{digest}\"}}",
    );
    let output = expect_ok(
        fixture,
        "download_location",
        &envelope,
        "download-location-ok",
        "DOWNLOAD-LOCATION-OK",
    )?;
    require(
        &output,
        &format!("/sha256/abcd/blobs/{digest}"),
        "download location acceptance",
    )?;
    let envelope = "{\"base\":\"https://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/\",\"location\":\"/v2/tailrocks/velnor-job/blobs/uploads/01234567\"}";
    let output = expect_ok(
        fixture,
        "upload_location",
        envelope,
        "upload-location-ok",
        "UPLOAD-LOCATION-OK",
    )?;
    require(
        &output,
        "https://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/01234567",
        "upload location acceptance",
    )?;
    let envelope = format!(
        "{{\"location\":\"https://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/01234567?foo=1\",\"digest\":\"{digest}\"}}",
    );
    let output = expect_ok(
        fixture,
        "upload_target",
        &envelope,
        "upload-target-ok",
        "UPLOAD-TARGET-OK",
    )?;
    require(
        &output,
        &format!("foo=1&digest={}", urlencoding(&digest)),
        "upload target acceptance",
    )?;
    Ok(())
}

pub(crate) fn urlencoding(value: &str) -> String {
    value.replace(':', "%3A")
}

pub(crate) fn check_accepts_registry_auth(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let envelope = "{\"value\":\"Bearer realm=\\\"https://ghcr.io/token\\\",service=\\\"ghcr.io\\\",scope=\\\"repository:tailrocks/velnor-job:pull,push\\\"\"}";
    let output = expect_ok(
        fixture,
        "challenge",
        envelope,
        "challenge-ok",
        "CHALLENGE-OK",
    )?;
    require(
        &output,
        "realm=https://ghcr.io/token",
        "challenge acceptance",
    )?;
    require(&output, "service=ghcr.io", "challenge acceptance")?;
    let envelope = "{\"params\":{\"realm\":\"https://ghcr.io/token\",\"service\":\"ghcr.io\",\"scope\":\"repository:tailrocks/velnor-job:pull,push\"}}";
    let output = expect_ok(
        fixture,
        "token_url",
        envelope,
        "token-url-ok",
        "TOKEN-URL-OK",
    )?;
    require(
        &output,
        "https://ghcr.io/token?service=ghcr.io&scope=repository%3Atailrocks%2Fvelnor-job%3Apull%2Cpush",
        "token url acceptance",
    )?;
    let payload = "{\"token\":\"AbcDEF1234567890._~- Tess\"}".replace(' ', "");
    let envelope = format!("{{\"payload_hex\":\"{}\"}}", hex_encode(payload.as_bytes()));
    expect_ok(fixture, "token", &envelope, "token-ok", "TOKEN-OK")?;
    let digest = format!("sha256:{}", sha256_hex(b"registry-head-bytes"));
    let envelope = format!(
        concat!(
            "{{\"status\":200,\"headers\":{{\"docker-content-digest\":\"{digest}\",",
            "\"content-length\":\"19\"}},\"digest\":\"{digest}\",\"size\":19}}"
        ),
        digest = digest,
    );
    let output = expect_ok(fixture, "head", &envelope, "head-ok", "HEAD-OK")?;
    require(&output, "present=True", "head acceptance")?;
    let envelope = format!(
        concat!("{{\"status\":404,\"headers\":{{}},\"digest\":\"{digest}\",\"size\":19}}"),
        digest = digest,
    );
    let output = expect_ok(fixture, "head", &envelope, "head-404", "HEAD-OK")?;
    require(&output, "present=False", "head absence acceptance")?;
    Ok(())
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
