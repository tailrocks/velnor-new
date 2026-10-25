use super::assets::{sdk_asset_size_limits, target_binary_digests, validate_manifest_identity};
use super::{MAX_CALLEE_BYTES, SDK_REPOSITORY, SDK_REPOSITORY_ID, compare_stable_versions};
use std::time::Duration;
use velnor_actions_contract::{ReleaseManifest, ReleaseTarget};
use velnor_actions_mise::CheckDeadline;

#[test]
fn published_callee_reader_limit_passes_production_selection_validation() {
    assert_eq!(
        MAX_CALLEE_BYTES,
        velnor_actions_contract::package_release_policy::MAX_CARGO_VERSION_MANIFEST_BYTES
    );
    let source_sha = "a".repeat(40);
    let expired = CheckDeadline::after(Duration::ZERO).expect("expired deadline");
    let error = crate::package_release_publish::read_protected_source_file(
        SDK_REPOSITORY,
        SDK_REPOSITORY_ID,
        "main",
        &source_sha,
        ".github/workflows/homebrew-adoption.yml",
        MAX_CALLEE_BYTES,
        expired,
    )
    .expect_err("the valid callee limit reaches the deadline before any network request");
    assert_eq!(
        error.to_string(),
        "internal: package_public_verification_deadline"
    );

    let error = crate::package_release_publish::read_protected_source_file(
        SDK_REPOSITORY,
        SDK_REPOSITORY_ID,
        "main",
        &source_sha,
        ".github/workflows/homebrew-adoption.yml",
        MAX_CALLEE_BYTES + 1,
        expired,
    )
    .expect_err("the production reader must reject a caller limit above its ceiling");
    assert_eq!(
        error.to_string(),
        "internal: package_public_source_file_selection_invalid"
    );
}

#[test]
fn sdk_binding_accepts_only_manifest_source_and_release_tag_agreement() {
    let bytes = include_bytes!("../../../fixtures/consumer-release-manifest.json");
    let text = std::str::from_utf8(bytes).expect("manifest fixture UTF-8");
    let manifest = ReleaseManifest::parse_json_with_limit(text, "fixture", 8 * 1024 * 1024)
        .expect("typed manifest");
    manifest
        .validate("fixture")
        .expect("valid fixture manifest");
    assert_eq!(manifest.repository, SDK_REPOSITORY);
    assert_eq!(manifest.version, "0.1.4");
    assert_eq!(manifest.commit, "b".repeat(40));
    validate_manifest_identity(&manifest, "0.1.4", &"b".repeat(40), &"b".repeat(40))
        .expect("matching immutable tag and manifest");
    assert!(
        validate_manifest_identity(&manifest, "0.1.4", &"c".repeat(40), &"b".repeat(40)).is_err()
    );
    assert!(
        validate_manifest_identity(&manifest, "0.1.4", &"b".repeat(40), &"c".repeat(40)).is_err()
    );
    assert!(
        validate_manifest_identity(&manifest, "0.1.5", &"b".repeat(40), &"b".repeat(40)).is_err()
    );
}

#[test]
fn sdk_release_inventory_is_closed_and_version_gate_is_numeric() {
    let limits = sdk_asset_size_limits("0.1.53");
    assert_eq!(limits.len(), 20);
    for target in ReleaseTarget::ALL {
        let binary = format!("velnor-actions-0.1.53-{}", target.triple());
        assert!(limits.contains_key(&binary));
        assert!(limits.contains_key(&format!("{binary}.sha256")));
        assert!(limits.contains_key(&format!("{binary}.provenance.json")));
        assert!(limits.contains_key(&format!("{binary}.intoto.jsonl")));
        assert!(limits.contains_key(&format!("{binary}.sha256.intoto.jsonl")));
        assert!(limits.contains_key(&format!("{binary}.provenance.json.intoto.jsonl")));
    }
    assert!(limits.contains_key("release-manifest.json"));
    assert!(limits.contains_key("release-manifest.json.intoto.jsonl"));
    assert!(
        compare_stable_versions("0.1.53", "0.1.53")
            .is_ok_and(|ordering| ordering == std::cmp::Ordering::Equal)
    );
    assert!(
        compare_stable_versions("0.1.54", "0.1.53")
            .is_ok_and(|ordering| ordering == std::cmp::Ordering::Greater)
    );
    assert!(
        compare_stable_versions("0.2.0", "0.1.99")
            .is_ok_and(|ordering| ordering == std::cmp::Ordering::Greater)
    );
    assert!(
        compare_stable_versions("1.0.0", "2.0.0")
            .is_ok_and(|ordering| ordering == std::cmp::Ordering::Less)
    );
}

#[test]
fn mac_host_and_linux_seed_runner_use_distinct_manifest_digests() {
    let mut value: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/consumer-release-manifest.json"
    ))
    .expect("manifest fixture");
    let targets = value["targets"].as_array_mut().expect("target records");
    for target in targets {
        let digest = match target["target"].as_str() {
            Some("x86_64-unknown-linux-gnu") => "1",
            Some("aarch64-apple-darwin") => "2",
            Some("x86_64-apple-darwin") => "3",
            _ => panic!("unexpected fixture target"),
        };
        target["sha256"] = serde_json::Value::String(digest.repeat(64));
    }
    let manifest: ReleaseManifest = serde_json::from_value(value).expect("typed manifest");
    let (host, linux_seed_runner) =
        target_binary_digests(&manifest, ReleaseTarget::MacosArm64).expect("both target pins");
    assert_eq!(host, "2".repeat(64));
    assert_eq!(linux_seed_runner, "1".repeat(64));
    assert_ne!(host, linux_seed_runner);
}

#[cfg(unix)]
#[test]
fn temporary_sdk_attestation_assets_are_private_at_creation() {
    use std::os::unix::fs::MetadataExt;

    let directory = tempfile::tempdir().expect("temporary parent");
    let path = super::assets::write_private_temp_asset(
        directory.path(),
        "release-manifest.json",
        b"manifest",
    )
    .expect("private asset");
    let mode = std::fs::metadata(path).expect("asset metadata").mode() & 0o777;
    assert_eq!(mode, 0o600);
}
