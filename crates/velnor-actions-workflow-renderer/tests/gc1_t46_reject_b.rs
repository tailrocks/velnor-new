// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_rejects_registry_locations(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let digest = format!("sha256:{}", "c".repeat(64));
    for (stem, location, message) in [
        (
            "download-scheme",
            format!("http://pkg-containers.githubusercontent.com/a/blobs/{digest}"),
            "Registry download URL is outside the permitted HTTPS location.",
        ),
        (
            "download-host",
            format!("https://example.com/a/blobs/{digest}"),
            "Registry download URL is outside the permitted HTTPS location.",
        ),
        (
            "download-userinfo",
            format!("https://user@pkg-containers.githubusercontent.com/a/blobs/{digest}"),
            "Registry download URL is outside the permitted HTTPS location.",
        ),
        (
            "download-backslash",
            format!("https://pkg-containers.githubusercontent.com/a\\/blobs/{digest}"),
            "Registry download URL has an incorrect character.",
        ),
        (
            "download-suffix",
            "https://pkg-containers.githubusercontent.com/a/blobs/sha256:deadbeef".to_owned(),
            "Registry download URL is outside the permitted HTTPS location.",
        ),
    ] {
        let envelope = format!(
            "{{\"location\":{},\"digest\":\"{digest}\"}}",
            json_string(&location),
            digest = digest
        );
        expect_rejection(
            fixture,
            "download_location",
            &envelope,
            stem,
            "DOWNLOAD-LOCATION-REJECTED",
            Some(message),
        )?;
    }
    let envelope = format!(
        concat!(
            "{{\"path\":\"/v2/tailrocks/velnor-job/manifests/{digest}\",",
            "\"digest\":\"{digest}\",\"status\":302,",
            "\"location\":\"https://pkg-containers.githubusercontent.com/a/blobs/{digest}\"}}"
        ),
        digest = digest,
    );
    expect_rejection(
        fixture,
        "redirect",
        &envelope,
        "redirect-manifest",
        "REDIRECT-REJECTED",
        Some("Registry manifest redirects are not permitted."),
    )?;
    check_rejects_upload_locations(fixture, &digest)?;
    Ok(())
}

pub(crate) fn check_rejects_upload_locations(
    fixture: &Fixture,
    digest: &str,
) -> Result<(), Box<dyn Error>> {
    for (stem, base, location, message) in [
        (
            "upload-host",
            "https://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/",
            "https://example.com/v2/tailrocks/velnor-job/blobs/uploads/1",
            "Registry upload location is outside the fixed HTTPS upload path.",
        ),
        (
            "upload-traversal",
            "https://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/",
            "/v2/tailrocks/velnor-job/blobs/uploads/../manifests/x",
            "Registry upload location is outside the fixed HTTPS upload path.",
        ),
        (
            "upload-userinfo",
            "https://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/",
            "https://user@ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/1",
            "Registry upload location is outside the fixed HTTPS upload path.",
        ),
    ] {
        let envelope = format!(
            "{{\"base\":{},\"location\":{}}}",
            json_string(base),
            json_string(location),
        );
        expect_rejection(
            fixture,
            "upload_location",
            &envelope,
            stem,
            "UPLOAD-LOCATION-REJECTED",
            Some(message),
        )?;
    }
    let envelope = format!(
        "{{\"location\":\"https://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/1?digest={digest}\",\"digest\":\"{digest}\"}}",
    );
    expect_rejection(
        fixture,
        "upload_target",
        &envelope,
        "upload-target-digest",
        "UPLOAD-TARGET-REJECTED",
        Some("Registry upload location contains a digest parameter."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_registry_auth(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    for (stem, value, message) in [
        (
            "challenge-basic",
            "Basic abcdefgh",
            "Registry authentication challenge has an incorrect format.",
        ),
        (
            "challenge-realm",
            "Bearer realm=\"https://example.com/token\",service=\"ghcr.io\",scope=\"x\"",
            "Registry authentication challenge has an incorrect format.",
        ),
        (
            "challenge-dup",
            "Bearer realm=\"https://ghcr.io/token\",realm=\"https://ghcr.io/token\"",
            "Registry authentication challenge has an incorrect format.",
        ),
    ] {
        let envelope = format!("{{\"value\":{}}}", json_string(value));
        expect_rejection(
            fixture,
            "challenge",
            &envelope,
            stem,
            "CHALLENGE-REJECTED",
            Some(message),
        )?;
    }
    for (stem, payload, message) in [
        (
            "token-missing",
            "{\"other\":\"AbcDEF1234567890._~-=\"}",
            "Registry token response has no token in the required format.",
        ),
        (
            "token-short",
            "{\"token\":\"short\"}",
            "Registry token response has no token in the required format.",
        ),
        (
            "token-chars",
            "{\"token\":\"not a token at all!!!!\"}",
            "Registry token response has no token in the required format.",
        ),
    ] {
        let envelope = format!("{{\"payload_hex\":\"{}\"}}", hex_encode(payload.as_bytes()));
        expect_rejection(
            fixture,
            "token",
            &envelope,
            stem,
            "TOKEN-REJECTED",
            Some(message),
        )?;
    }
    check_rejects_head_responses(fixture)?;
    Ok(())
}

pub(crate) fn check_rejects_head_responses(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let digest = format!("sha256:{}", sha256_hex(b"registry-head-bytes"));
    let envelope = format!(
        concat!("{{\"status\":500,\"headers\":{{}},\"digest\":\"{digest}\",\"size\":19}}"),
        digest = digest,
    );
    expect_rejection(
        fixture,
        "head",
        &envelope,
        "head-status",
        "HEAD-REJECTED",
        Some("Registry digest HEAD verification failed."),
    )?;
    let envelope = format!(
        concat!(
            "{{\"status\":200,\"headers\":{{\"docker-content-digest\":\"{digest}\",",
            "\"content-length\":\"20\"}},\"digest\":\"{digest}\",\"size\":19}}"
        ),
        digest = digest,
    );
    expect_rejection(
        fixture,
        "head",
        &envelope,
        "head-size",
        "HEAD-REJECTED",
        Some("Registry size HEAD verification failed."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_retrieval(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let body = b"registry-get-bytes";
    let digest = format!("sha256:{}", sha256_hex(body));
    let body_hex = hex_encode(body);
    let tampered_hex = hex_encode(b"registry-get-bytez");
    let fill = |body: &str, expected: &str| {
        format!(
            concat!(
                "{{\"status\":200,\"headers\":{{\"docker-content-digest\":\"{digest}\",",
                "\"content-length\":\"{len}\"}},\"body_hex\":\"{body}\",",
                "\"digest\":\"{digest}\",\"size\":{len},\"expected_hex\":\"{expected}\"}}"
            ),
            digest = digest,
            len = body.len() / 2,
            body = body,
            expected = expected,
        )
    };
    expect_rejection(
        fixture,
        "get",
        &fill(&body_hex, &tampered_hex),
        "get-expected",
        "GET-REJECTED",
        Some("Registry retrieved bytes changed during publication."),
    )?;
    expect_rejection(
        fixture,
        "get",
        &fill(&tampered_hex, &tampered_hex),
        "get-digest",
        "GET-REJECTED",
        Some("Registry response bytes have an incorrect digest or size."),
    )?;
    let envelope = format!(
        concat!(
            "{{\"status\":200,\"headers\":{{\"docker-content-digest\":\"{digest}\",",
            "\"content-length\":\"99\"}},\"body_hex\":\"{body}\",",
            "\"digest\":\"{digest}\",\"size\":{len},\"expected_hex\":\"{body}\"}}"
        ),
        digest = digest,
        body = body_hex,
        len = body.len(),
    );
    expect_rejection(
        fixture,
        "get",
        &envelope,
        "get-size",
        "GET-REJECTED",
        Some("Registry GET size is different from the specified size."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_publication(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let (_config, config_hex, data, data_hex) = blob_case();
    let (manifest, manifest_hex) = manifest_case();
    let manifest_text = String::from_utf8(manifest.clone())?;
    let (archive, _, _, _, _) = archive_case();
    let archive_path = write_archive(fixture, "reject-blobs.oci.tar", &archive)?;
    let archive_json = json_string(&archive_path);
    let manifest_digest = format!("sha256:{manifest_hex}");
    expect_rejection(
        fixture,
        "descriptors",
        "{\"manifest\":{}}",
        "descriptors-shape",
        "DESCRIPTORS-REJECTED",
        Some("OCI manifest blob descriptors are incorrect."),
    )?;
    let conflict = replace_once(
        &manifest_text,
        &format!("\"digest\":\"sha256:{data_hex}\""),
        &format!("\"digest\":\"sha256:{config_hex}\""),
    )?;
    expect_rejection(
        fixture,
        "descriptors",
        &format!("{{\"manifest\":{conflict}}}"),
        "descriptors-conflict",
        "DESCRIPTORS-REJECTED",
        Some("Repeated OCI blob digest has inconsistent sizes."),
    )?;
    let bad_digest = replace_once(
        &manifest_text,
        &format!("\"digest\":\"sha256:{config_hex}\""),
        "\"digest\":\"sha256:ZZZ\"",
    )?;
    expect_rejection(
        fixture,
        "descriptors",
        &format!("{{\"manifest\":{bad_digest}}}"),
        "descriptors-digest",
        "DESCRIPTORS-REJECTED",
        Some("OCI blob descriptor is incorrect."),
    )?;
    check_rejects_manifest_reads(
        fixture,
        &archive_json,
        &manifest,
        &manifest_digest,
        &data,
        &data_hex,
    )?;
    Ok(())
}
