// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::image_publish::challenge;
use velnor_ci::image_publish::digest::Digest;
use velnor_ci::image_publish::location::{
    DOWNLOAD_HOST, Kind, download, fixed, object, upload, with_digest,
};

const HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn digest() -> Digest {
    Digest::parse(&format!("sha256:{HEX}")).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn only_a_sha256_digest_addresses_a_registry_object() {
    for tag in [
        "latest",
        "main",
        "v1",
        "sha256:abc",
        &format!("sha256:{}", HEX.to_uppercase()),
    ] {
        assert!(Digest::parse(tag).is_err(), "{tag}");
    }
    let url = object(Kind::Manifests, &digest());
    assert_eq!(
        url,
        format!("https://ghcr.io/v2/tailrocks/velnor-job/manifests/sha256:{HEX}")
    );
}

#[test]
fn the_upload_location_must_stay_on_the_fixed_upload_path() {
    let state = "/v2/tailrocks/velnor-job/blobs/uploads/abc?_state=xyz";
    assert_eq!(
        upload(state).as_deref(),
        Ok(format!("https://ghcr.io{state}").as_str())
    );
    let absolute = format!("https://ghcr.io:443{state}");
    assert_eq!(upload(&absolute).as_deref(), Ok(absolute.as_str()));
    for refused in [
        "",
        "uploads/abc",
        "//evil.example/v2/tailrocks/velnor-job/blobs/uploads/a",
        "https://evil.example/v2/tailrocks/velnor-job/blobs/uploads/a",
        "https://ghcr.io@evil.example/v2/tailrocks/velnor-job/blobs/uploads/a",
        "https://user@ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/a",
        "https://ghcr.io:444/v2/tailrocks/velnor-job/blobs/uploads/a",
        "http://ghcr.io/v2/tailrocks/velnor-job/blobs/uploads/a",
        "/v2/tailrocks/velnor-job/blobs/uploads/a#frag",
        "/v2/other/name/blobs/uploads/a",
        "/v2/tailrocks/velnor-job/blobs/uploads/../../../x",
        "/v2/tailrocks/velnor-job/blobs/uploads/%2e%2e/x",
        "/v2/tailrocks/velnor-job/blobs/uploads/a%0d%0ax",
        "/v2/tailrocks/velnor-job/blobs/uploads/a\\b",
        "/v2/tailrocks/velnor-job/blobs/uploads/a?=x",
    ] {
        assert!(upload(refused).is_err(), "{refused}");
    }
}

#[test]
fn the_digest_is_appended_once_and_never_taken_from_the_server() {
    let with = with_digest("https://ghcr.io/v2/x?_state=a", &digest());
    assert_eq!(
        with.as_deref(),
        Ok(format!("https://ghcr.io/v2/x?_state=a&digest=sha256%3A{HEX}").as_str())
    );
    let bare = with_digest("https://ghcr.io/v2/x", &digest());
    assert_eq!(
        bare.as_deref(),
        Ok(format!("https://ghcr.io/v2/x?digest=sha256%3A{HEX}").as_str())
    );
    assert!(with_digest("https://ghcr.io/v2/x?digest=sha256:z", &digest()).is_err());
}

#[test]
fn a_blob_redirect_may_only_reach_the_package_download_host() {
    let good = format!("https://{DOWNLOAD_HOST}/ghcr1/blobs/sha256:{HEX}?se=1&sig=a%2Fb");
    assert_eq!(download(&good, &digest()).as_deref(), Ok(good.as_str()));
    let other = Digest::parse(&format!("sha256:{}", "f".repeat(64)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    for refused in [
        format!("https://evil.example/ghcr1/blobs/sha256:{HEX}"),
        format!("https://ghcr.io/ghcr1/blobs/sha256:{HEX}"),
        format!("http://{DOWNLOAD_HOST}/ghcr1/blobs/sha256:{HEX}"),
        format!("https://{DOWNLOAD_HOST}/ghcr1/blobs/sha256:{HEX}#x"),
        format!("https://{DOWNLOAD_HOST}/ghcr1/blobs/sha256:{HEX}\n"),
        format!(
            "https://{DOWNLOAD_HOST}/ghcr1/blobs/sha256:{}",
            "f".repeat(64)
        ),
    ] {
        assert!(download(&refused, &digest()).is_err(), "{refused}");
    }
    assert!(download(&good, &other).is_err());
    assert!(fixed(&good, DOWNLOAD_HOST).is_ok());
    assert!(fixed(&good, "ghcr.io").is_err());
}

#[test]
fn the_authentication_challenge_must_name_the_fixed_token_endpoint() {
    let real = r#"Bearer realm="https://ghcr.io/token",service="ghcr.io",scope="repository:user/image:pull""#;
    assert!(challenge::verify(real).is_ok());
    for refused in [
        "Basic realm=\"https://ghcr.io/token\"",
        "Bearer",
        r#"Bearer realm="https://evil.example/token",service="ghcr.io""#,
        r#"Bearer realm="https://ghcr.io/token",service="other""#,
        r#"Bearer realm="https://ghcr.io/token",service="ghcr.io",service="ghcr.io""#,
        r#"Bearer realm="https://ghcr.io/token",service="ghcr.io",extra="x""#,
        r#"Bearer realm="https://ghcr.io/token" service="ghcr.io""#,
    ] {
        assert!(challenge::verify(refused).is_err(), "{refused}");
    }
}

#[test]
fn a_registry_token_has_the_token_format_and_the_exchange_asks_for_push() {
    let token = challenge::token(br#"{"token":"abcdefghijklmnop.q-r_s~t="}"#);
    assert_eq!(
        token.as_ref().map(|token| token.as_str()).ok(),
        Some("abcdefghijklmnop.q-r_s~t=")
    );
    let fallback = challenge::token(br#"{"access_token":"abcdefghijklmnop1234"}"#);
    assert!(fallback.is_ok());
    for refused in [
        r#"{"token":"short"}"#,
        r#"{"token":"abcdefghijklmnop with space"}"#,
        r#"{"token":5}"#,
        r"{}",
        r#"{"token":"abcdefghijklmnop1234","token":"abcdefghijklmnop1234"}"#,
        "not json",
    ] {
        assert!(challenge::token(refused.as_bytes()).is_err(), "{refused}");
    }
    assert_eq!(
        challenge::token_url(),
        "https://ghcr.io/token?service=ghcr.io&scope=repository%3Atailrocks%2Fvelnor-job%3Apull%2Cpush"
    );
    assert_eq!(
        challenge::basic("me", "secret").as_str(),
        "Basic bWU6c2VjcmV0"
    );
}
