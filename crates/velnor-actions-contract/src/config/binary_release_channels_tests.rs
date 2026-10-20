use super::{RustBinaryReleaseConfig, tests::enabled_preview};

#[test]
fn both_channels_share_exactly_one_qualified_inventory() {
    for preview in [false, true] {
        for stable in [false, true] {
            let mut config = enabled_preview();
            config.preview.enabled = preview;
            config.stable.enabled = stable;
            assert_eq!(config.enabled(), preview || stable);
            assert!(config.validate("config.toml").is_ok());
            assert_eq!(config.expected_asset_names().len(), 13);
        }
    }
}

#[test]
fn rolling_requires_an_existing_release_but_stable_does_not() {
    let mut config = enabled_preview();
    config.preview.release_id = 0;
    let error = config
        .validate("config.toml")
        .expect_err("rolling identity missing");
    assert!(
        error
            .to_string()
            .contains("existing_rolling_release_required")
    );
    config.preview.enabled = false;
    config.stable.enabled = true;
    assert!(config.validate("config.toml").is_ok());
    config.stable.environment = "preview-publish".to_owned();
    assert!(config.validate("config.toml").is_err());
}

#[test]
fn source_version_owner_must_be_one_explicit_product() {
    for package in ["", "unknown", "jackin;id"] {
        let mut config = enabled_preview();
        config.version_package = package.to_owned();
        assert!(config.validate("config.toml").is_err(), "{package:?}");
    }
}

#[test]
fn removed_single_binary_and_preview_inventory_shapes_are_rejected() {
    for (field, value) in [
        ("enabled", serde_json::json!(true)),
        ("package", serde_json::json!("jackin")),
        ("bin", serde_json::json!("jackin")),
    ] {
        let mut json = serde_json::to_value(enabled_preview()).expect("config");
        json[field] = value;
        assert!(
            serde_json::from_value::<RustBinaryReleaseConfig>(json).is_err(),
            "{field}"
        );
    }
    for field in [
        "producer",
        "products",
        "companions",
        "version_env",
        "evidence",
    ] {
        let mut json = serde_json::to_value(enabled_preview()).expect("config");
        json["preview"][field] = json[field].take();
        assert!(
            serde_json::from_value::<RustBinaryReleaseConfig>(json).is_err(),
            "{field}"
        );
    }
}

#[test]
fn legacy_bootstrap_requires_explicit_strict_fingerprint_and_enabled_release_binding() {
    let mut config = enabled_preview();
    assert!(config.preview.bootstrap_snapshot_sha256.is_none());
    assert!(
        !serde_json::to_value(&config).expect("config")["preview"]
            .as_object()
            .expect("preview")
            .contains_key("bootstrap_snapshot_sha256")
    );
    config.preview.bootstrap_snapshot_sha256 = Some("ab".repeat(32));
    assert!(config.validate("config.toml").is_ok());
    for digest in [
        "A".repeat(64),
        "g".repeat(64),
        "a".repeat(63),
        "a".repeat(65),
        String::new(),
    ] {
        config.preview.bootstrap_snapshot_sha256 = Some(digest);
        assert!(config.validate("config.toml").is_err());
    }
    config.preview.bootstrap_snapshot_sha256 = Some("ab".repeat(32));
    config.preview.enabled = false;
    assert!(config.validate("config.toml").is_err());
    config.preview.enabled = true;
    config.preview.release_id = 0;
    assert!(config.validate("config.toml").is_err());
}
