use reqwest::header::{ACCEPT, AUTHORIZATION};
use serde_json::{Value, json};

use super::response::{attestation_path, parse_attestations, parse_release};
use super::{api_request, api_url, asset_redirect_request, client, valid_pat, validate_asset_redirect};
use crate::artifact_admission::manifest::{ARCHIVE_ASSET, CHECKSUM_ASSET, MANIFEST_ASSET};
use crate::error::HostError;

const SOURCE: &str = "11abcdef11abcdef11abcdef11abcdef11abcdef";
const ASSET_DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn asset(name: &str, id: u64) -> Value {
    json!({"id": id, "name": name, "state": "uploaded", "size": 1, "digest": ASSET_DIGEST})
}

fn release_response(source: &str) -> Value {
    json!({
        "tag_name": format!("runner-{source}"),
        "target_commitish": source,
        "draft": false,
        "prerelease": false,
        "immutable": true,
        "assets": [
            asset("velnor-runner-linux-amd64.tar", 1),
            asset("velnor-dind-linux-amd64.tar", 2),
            asset(ARCHIVE_ASSET, 3),
            asset(MANIFEST_ASSET, 4),
            asset(CHECKSUM_ASSET, 5),
        ],
    })
}

fn bundle() -> Value {
    json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "dsseEnvelope": {},
        "verificationMaterial": {},
    })
}

fn attestation_response() -> Value {
    json!({"attestations": [{
        "repository_id": 1_390_620_900,
        "repository": "tailrocks/velnor-new",
        "bundle": bundle(),
    }]})
}

#[test]
fn accepts_only_the_exact_immutable_release_and_asset_set() -> Result<(), HostError> {
    let assets = parse_release(SOURCE, release_response(SOURCE))?;
    assert_eq!(assets.len(), 5);
    assert!(assets.contains_key(ARCHIVE_ASSET));
    assert!(assets.contains_key(MANIFEST_ASSET));
    assert!(assets.contains_key(CHECKSUM_ASSET));
    Ok(())
}

#[test]
fn rejects_release_identity_and_publication_state_mismatches() {
    for (field, value) in [
        ("tag_name", json!("runner-wrong")),
        ("target_commitish", json!("22abcdef22abcdef22abcdef22abcdef22abcdef")),
        ("draft", json!(true)),
        ("prerelease", json!(true)),
        ("immutable", json!(false)),
    ] {
        let mut response = release_response(SOURCE);
        response[field] = value;
        assert_eq!(parse_release(SOURCE, response), Err(HostError::Identity));
    }
}

#[test]
fn rejects_missing_extra_duplicate_or_invalid_assets() {
    let mut missing = release_response(SOURCE);
    if let Some(assets) = missing["assets"].as_array_mut() {
        assets.pop();
    }
    let mut extra = release_response(SOURCE);
    if let Some(assets) = extra["assets"].as_array_mut() {
        assets.push(asset("unexpected.tar", 6));
    }
    let mut duplicate = release_response(SOURCE);
    if let Some(assets) = duplicate["assets"].as_array_mut() {
        assets.push(asset(ARCHIVE_ASSET, 6));
    }
    let mut invalid_id = release_response(SOURCE);
    invalid_id["assets"][0]["id"] = json!(0);
    let mut invalid_state = release_response(SOURCE);
    invalid_state["assets"][0]["state"] = json!("new");
    let mut invalid_digest = release_response(SOURCE);
    invalid_digest["assets"][0]["digest"] = json!("sha256:ABC");
    for response in [missing, extra, duplicate, invalid_id, invalid_state, invalid_digest] {
        assert_eq!(parse_release(SOURCE, response), Err(HostError::Identity));
    }
}

#[test]
fn accepts_only_product_repository_bundles_with_required_envelope_fields()
-> Result<(), HostError> {
    let response = attestation_response();
    let bundles = parse_attestations(&response)?;
    assert_eq!(bundles, vec![bundle()]);
    Ok(())
}

#[test]
fn rejects_foreign_or_malformed_attestation_rows() {
    let mut foreign_id = attestation_response();
    foreign_id["attestations"][0]["repository_id"] = json!(1);
    let mut foreign_repo = attestation_response();
    foreign_repo["attestations"][0]["repository"] = json!("elsewhere/project");
    let mut wrong_media = attestation_response();
    wrong_media["attestations"][0]["bundle"]["mediaType"] = json!("other");
    let mut missing_dsse = attestation_response();
    assert!(missing_dsse["attestations"][0]["bundle"]
        .as_object_mut()
        .is_some_and(|object| object.remove("dsseEnvelope").is_some()));
    let mut missing_material = attestation_response();
    assert!(missing_material["attestations"][0]["bundle"]
        .as_object_mut()
        .is_some_and(|object| object.remove("verificationMaterial").is_some()));
    let empty = json!({"attestations": []});
    let too_many = json!({"attestations": vec![attestation_response()["attestations"][0].clone(); 17]});
    for response in [
        foreign_id,
        foreign_repo,
        wrong_media,
        missing_dsse,
        missing_material,
        empty,
        too_many,
    ] {
        assert_eq!(parse_attestations(&response), Err(HostError::Identity));
    }
}

#[test]
fn requires_a_canonical_checksum_digest_for_attestation_lookup() -> Result<(), HostError> {
    let digest = format!("sha256:{}", "a".repeat(64));
    assert_eq!(
        attestation_path(&digest)?,
        format!("repos/tailrocks/velnor-new/attestations/{digest}")
    );
    for invalid in ["sha256:ABC", "sha512:abc", "sha256:abcd"] {
        assert_eq!(attestation_path(invalid), Err(HostError::Identity));
    }
    Ok(())
}

#[test]
fn api_requests_keep_the_fixed_origin_and_required_headers() -> Result<(), HostError> {
    assert_eq!(
        api_url("repos/tailrocks/velnor-new/releases").map(|url| url.origin().ascii_serialization()),
        Ok("https://api.github.com".to_owned())
    );
    assert_eq!(api_url("//attacker.example/path"), Err(HostError::Identity));
    let client = client()?;
    let request = api_request(&client, "ghp_test-token", "repos/tailrocks/velnor-new/releases")
        .and_then(|builder| builder.build().map_err(|_| HostError::Identity))?;
    assert_eq!(request.headers().get(ACCEPT).and_then(|value| value.to_str().ok()), Some("application/vnd.github+json"));
    assert_eq!(request.headers().get("X-GitHub-Api-Version").and_then(|value| value.to_str().ok()), Some("2022-11-28"));
    assert!(request.headers().contains_key(AUTHORIZATION));
    assert!(request.url().as_str().starts_with("https://api.github.com/repos/"));
    Ok(())
}

#[test]
fn validates_nonempty_bounded_control_free_api_credentials() {
    assert!(valid_pat("ghp_test-token"));
    assert!(!valid_pat(""));
    assert!(!valid_pat("token\nvalue"));
    assert!(!valid_pat(&"x".repeat(4097)));
}

#[test]
fn permits_only_the_fixed_https_asset_origin() -> Result<(), HostError> {
    let url = validate_asset_redirect(
        "https://release-assets.githubusercontent.com/releases/download/x?token=secret",
    )?;
    if url.host_str() != Some("release-assets.githubusercontent.com") {
        return Err(HostError::Identity);
    }
    Ok(())
}

#[test]
fn rejects_untrusted_or_credentialed_redirects() {
    for location in [
        "http://release-assets.githubusercontent.com/x",
        "https://github.com/x",
        "https://user@release-assets.githubusercontent.com/x",
        "https://release-assets.githubusercontent.com:444/x",
        "https://release-assets.githubusercontent.com/x#fragment",
    ] {
        assert_eq!(validate_asset_redirect(location), Err(HostError::Identity));
    }
}

#[test]
fn asset_redirect_request_never_carries_the_api_credential() -> Result<(), HostError> {
    let client = client()?;
    let url = validate_asset_redirect(
        "https://release-assets.githubusercontent.com/releases/download/x?token=ephemeral",
    )?;
    let request = asset_redirect_request(&client, url)
        .build()
        .map_err(|_| HostError::Identity)?;
    assert!(!request.headers().contains_key(AUTHORIZATION));
    Ok(())
}
