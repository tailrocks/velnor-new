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
fn immutable_preview_needs_no_mutable_release_binding() {
    let mut config = enabled_preview();
    assert!(config.validate("config.toml").is_ok());
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
fn mutable_release_and_bootstrap_configuration_are_rejected() {
    for field in ["release_id", "bootstrap_snapshot_sha256"] {
        let mut value = serde_json::to_value(enabled_preview()).expect("config");
        value["preview"][field] = serde_json::json!(1);
        assert!(
            serde_json::from_value::<RustBinaryReleaseConfig>(value).is_err(),
            "{field}"
        );
    }
}
