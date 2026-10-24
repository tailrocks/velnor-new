use super::*;
use crate::config::{CheckSystemTool, CheckSystemToolKind, VerificationRunner};

#[test]
fn companion_names_must_survive_github_upload_without_renaming() {
    for renamed in [".hidden", "name.", ".", ".."] {
        let mut config = enabled_preview();
        config.companions[0].asset_name = renamed.to_owned();
        assert!(config.validate("config.toml").is_err());
    }
}

pub(super) fn enabled_preview() -> RustBinaryReleaseConfig {
    RustBinaryReleaseConfig {
        version_package: "jackin".to_owned(),
        preview: RustBinaryPreviewConfig {
            enabled: true,
            ..RustBinaryPreviewConfig::default()
        },
        version_env: "JACKIN_VERSION_OVERRIDE".to_owned(),
        producer: RustBinaryReleaseProducer {
            task: "release:preview-package".to_owned(),
            tools: vec!["rust".to_owned()],
            inputs: vec!["Cargo.lock".to_owned(), "mise.toml".to_owned()],
            ..RustBinaryReleaseProducer::default()
        },
        products: vec![RustBinaryReleaseProduct {
            id: "jackin".to_owned(),
            package: "jackin".to_owned(),
            bins: vec!["jackin".to_owned(), "jackin-role".to_owned()],
            targets: vec![
                "aarch64-apple-darwin".to_owned(),
                "x86_64-unknown-linux-gnu".to_owned(),
            ],
            archive: RustBinaryReleaseArchive::TarGz,
        }],
        companions: vec![RustBinaryReleaseCompanion {
            id: "capsule-manifest".to_owned(),
            path: "capsule/manifest.json".to_owned(),
            asset_name: "capsule-manifest.json".to_owned(),
        }],
        ..RustBinaryReleaseConfig::default()
    }
}

#[test]
fn preview_defaults_independently_disabled_with_mandatory_evidence() {
    let preview = RustBinaryReleaseConfig::default();
    assert!(!preview.enabled());
    assert_eq!(preview.producer.timeout_minutes, 5);
    assert_eq!(preview.publication_timeout_minutes.minutes(), 5);
    assert_eq!(preview.version_env, "");
    assert_eq!(
        preview.evidence.profile,
        RustBinaryReleaseEvidenceProfile::SignedSbomV1
    );
    assert!(preview.validate("config.toml").is_ok());
}

#[test]
fn publication_timeout_defaults_to_five_and_accepts_bounded_explicit_values() {
    let value = serde_json::to_value(enabled_preview()).expect("release config");
    let mut omitted = value.clone();
    omitted
        .as_object_mut()
        .expect("config object")
        .remove("publication_timeout_minutes");
    let omitted = serde_json::from_value::<RustBinaryReleaseConfig>(omitted)
        .expect("default publication timeout");
    assert_eq!(omitted.publication_timeout_minutes.minutes(), 5);
    for minutes in [1, 5, 15, 333] {
        let mut configured = value.clone();
        configured["publication_timeout_minutes"] = serde_json::json!(minutes);
        let config = serde_json::from_value::<RustBinaryReleaseConfig>(configured)
            .expect("typed publication timeout");
        assert_eq!(config.publication_timeout_minutes.minutes(), minutes);
        assert!(config.validate("config.toml").is_ok(), "{minutes}");
    }
}

#[test]
fn publication_timeout_rechecks_serde_bypassed_values_and_rejects_bad_shapes() {
    let value = serde_json::to_value(enabled_preview()).expect("release config");
    for minutes in [0, 334, 360, 361] {
        let mut configured = value.clone();
        configured["publication_timeout_minutes"] = serde_json::json!(minutes);
        let config = serde_json::from_value::<RustBinaryReleaseConfig>(configured)
            .expect("u16 can deserialize before validation");
        let error = config
            .validate("config.toml")
            .expect_err("invalid timeout rejected after deserialization");
        assert!(
            error
                .to_string()
                .contains("stacks.rust.binary_release.publication_timeout_minutes")
        );
    }
    for invalid in [
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!("15"),
        serde_json::json!(65536),
    ] {
        let mut configured = value.clone();
        configured["publication_timeout_minutes"] = invalid;
        assert!(serde_json::from_value::<RustBinaryReleaseConfig>(configured).is_err());
    }
    let serialized = serde_json::to_string(&value).expect("serialized config");
    let duplicate = format!(
        "{},\"publication_timeout_minutes\":15}}",
        serialized.strip_suffix('}').expect("object")
    );
    assert!(serde_json::from_str::<RustBinaryReleaseConfig>(&duplicate).is_err());
}

#[test]
fn enabled_preview_accepts_explicit_product_matrix_and_companion_assets() {
    let preview = enabled_preview();
    assert!(preview.validate("config.toml").is_ok());
    assert_eq!(preview.version_env, "JACKIN_VERSION_OVERRIDE");
    assert_eq!(
        preview.preview.versioning,
        RustBinaryPreviewVersioning::PackagePreviewCommitCountSha7
    );
}

#[test]
fn enabled_preview_requires_task_inputs_tools_and_products() {
    let mut preview = enabled_preview();
    preview.producer.inputs.clear();
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.producer.task.clear();
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.products.clear();
    assert!(preview.validate("config.toml").is_err());
}

#[test]
fn rejects_duplicate_or_unsafe_inventory_and_unbounded_producer_settings() {
    let mut preview = enabled_preview();
    preview.products[0]
        .targets
        .push("aarch64-apple-darwin".to_owned());
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.companions[0].path = "../formula.rb".to_owned();
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.producer.timeout_minutes = 361;
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.producer.build_jobs = 65;
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.companions[0].asset_name = "jackin-aarch64-apple-darwin.tar.gz".to_owned();
    assert!(preview.validate("config.toml").is_err());
}

#[test]
fn preview_version_variable_cannot_name_control_environment() {
    for value in [
        "GITHUB_TOKEN_VERSION_OVERRIDE",
        "RUST_VERSION_OVERRIDE",
        "bad-name",
    ] {
        let mut preview = enabled_preview();
        preview.version_env = value.to_owned();
        assert!(preview.validate("config.toml").is_err(), "{value}");
    }
}

#[test]
fn evidence_profile_has_no_boolean_escape_hatches() {
    let valid = serde_json::to_value(enabled_preview()).expect("release config");
    let mut invalid = valid.clone();
    invalid["evidence"]["checksums"] = serde_json::json!(false);
    assert!(serde_json::from_value::<RustBinaryReleaseConfig>(invalid).is_err());
    let config =
        serde_json::from_value::<RustBinaryReleaseConfig>(valid.clone()).expect("valid inventory");
    assert!(config.validate("config.toml").is_ok());
    let mut missing = valid.clone();
    missing
        .as_object_mut()
        .expect("config object")
        .remove("evidence");
    assert!(serde_json::from_value::<RustBinaryReleaseConfig>(missing).is_err());
    let mut missing = valid;
    missing["evidence"]
        .as_object_mut()
        .expect("evidence object")
        .remove("profile");
    assert!(serde_json::from_value::<RustBinaryReleaseConfig>(missing).is_err());
}

#[test]
fn rejects_unsupported_targets_manifest_escape_and_missing_source_input() {
    let mut preview = enabled_preview();
    preview.products[0].targets[0] = "x86_64-pc-windows-msvc".to_owned();
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.producer.manifest = "dist/other/output.json".to_owned();
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.producer.inputs.retain(|input| input != "mise.toml");
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.preview.environment = "consumer-binary-release".to_owned();
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview.companions[0].path = "jackin-aarch64-apple-darwin.tar.gz/child".to_owned();
    assert!(preview.validate("config.toml").is_err());
    let mut preview = enabled_preview();
    preview
        .producer
        .inputs
        .push("dist/preview/archive.tar.gz".to_owned());
    assert!(preview.validate("config.toml").is_err());
}

#[test]
fn producer_system_tool_pins_reuse_native_platform_validation() {
    let mut preview = enabled_preview();
    preview.producer.runner = VerificationRunner::MacosArm64;
    preview.producer.system_tools = vec![
        system_tool(
            CheckSystemToolKind::Swift,
            "6.4",
            "swiftlang-6.4.0.34.1 clang-2100.3.34.1",
        ),
        system_tool(CheckSystemToolKind::Xcode, "26.0", "16A242d"),
    ];
    assert!(preview.validate("config.toml").is_ok());

    let mut linux = enabled_preview();
    linux.producer.system_tools = preview.producer.system_tools.clone();
    assert!(linux.validate("config.toml").is_err());

    let mut duplicate = preview.clone();
    duplicate.producer.system_tools = vec![
        system_tool(
            CheckSystemToolKind::Swift,
            "6.4",
            "swiftlang-6.4.0.34.1 clang-2100.3.34.1",
        ),
        system_tool(
            CheckSystemToolKind::Swift,
            "6.4",
            "swiftlang-6.4.0.34.1 clang-2100.3.34.1",
        ),
    ];
    assert!(duplicate.validate("config.toml").is_err());

    let mut malformed = preview;
    malformed.producer.system_tools[0].version = "6;4".to_owned();
    assert!(malformed.validate("config.toml").is_err());
}

fn system_tool(kind: CheckSystemToolKind, version: &str, build: &str) -> CheckSystemTool {
    CheckSystemTool {
        kind,
        version: version.to_owned(),
        build: build.to_owned(),
        sdk_version: (kind == CheckSystemToolKind::Xcode).then(|| "27.0".to_owned()),
        developer_dir: (kind == CheckSystemToolKind::Xcode).then(|| {
            "/Applications/Xcode_27.1_Release_Candidate.app/Contents/Developer".to_owned()
        }),
    }
}

#[test]
fn jackin_product_inventory_derives_exact_29_asset_set() {
    let mut preview = enabled_preview();
    preview.products = vec![
        RustBinaryReleaseProduct {
            id: "jackin".to_owned(),
            package: "jackin".to_owned(),
            bins: vec![
                "jackin".to_owned(),
                "jackin-role".to_owned(),
                "jackin-usage-broker".to_owned(),
            ],
            targets: vec![
                "aarch64-apple-darwin".to_owned(),
                "x86_64-apple-darwin".to_owned(),
                "aarch64-unknown-linux-gnu".to_owned(),
                "x86_64-unknown-linux-gnu".to_owned(),
            ],
            archive: RustBinaryReleaseArchive::TarGz,
        },
        RustBinaryReleaseProduct {
            id: "jackin-capsule".to_owned(),
            package: "jackin-capsule".to_owned(),
            bins: vec!["jackin-capsule".to_owned()],
            targets: vec![
                "aarch64-unknown-linux-gnu".to_owned(),
                "x86_64-unknown-linux-gnu".to_owned(),
            ],
            archive: RustBinaryReleaseArchive::TarGz,
        },
    ];
    preview.companions = vec![RustBinaryReleaseCompanion {
        id: "capsule-manifest".to_owned(),
        path: "capsule-manifest.json".to_owned(),
        asset_name: "capsule-manifest.json".to_owned(),
    }];
    let assets = preview.expected_asset_names();
    assert_eq!(assets.len(), 29);
    assert_eq!(
        assets
            .iter()
            .filter(|name| name.ends_with(".tar.gz"))
            .count(),
        6
    );
    for name in [
        "jackin-aarch64-apple-darwin.tar.gz",
        "jackin-x86_64-apple-darwin.tar.gz",
        "jackin-aarch64-unknown-linux-gnu.tar.gz",
        "jackin-x86_64-unknown-linux-gnu.tar.gz",
        "jackin-capsule-aarch64-unknown-linux-gnu.tar.gz",
        "jackin-capsule-x86_64-unknown-linux-gnu.tar.gz",
        "SHA256SUMS",
        "capsule-manifest.json",
        "capsule-manifest.json.bundle",
        "release.json",
        "release.json.bundle",
    ] {
        assert!(assets.iter().any(|asset| asset == name), "{name}");
    }
    assert!(preview.validate("config.toml").is_ok());
}

#[test]
fn rejects_inventory_larger_than_canonical_receipt_limits() {
    let mut preview = enabled_preview();
    preview.products = vec![preview.products[0].clone(); 65];
    assert!(preview.validate("config.toml").is_err());

    let mut preview = enabled_preview();
    preview.products[0].targets = vec!["aarch64-apple-darwin".to_owned(); 257];
    assert!(preview.validate("config.toml").is_err());

    let mut preview = enabled_preview();
    preview.companions = (0..1021)
        .map(|index| RustBinaryReleaseCompanion {
            id: format!("asset-{index}"),
            path: format!("assets/{index}.json"),
            asset_name: format!("asset-{index}.json"),
        })
        .collect();
    assert!(preview.validate("config.toml").is_err());
}

#[test]
fn native_producer_timeout_has_independent_thirty_minute_ceiling() {
    assert!(crate::workflow::timeout::JobTimeout::new(360).is_ok());
    for minutes in [1, 5, 6, 10, 15, 20, 30] {
        let mut value = enabled_preview();
        value.producer.timeout_minutes = minutes;
        assert!(value.validate("config.toml").is_ok(), "{minutes}");
    }
    for minutes in [0, 31, 360, 361, u16::MAX] {
        let mut value = enabled_preview();
        value.producer.timeout_minutes = minutes;
        assert!(value.validate("config.toml").is_err(), "{minutes}");
    }
}

#[test]
fn native_producer_timeout_rejects_malformed_and_duplicate_values() {
    let producer = RustBinaryReleaseProducer::default();
    let canonical = serde_json::to_string(&producer).expect("producer JSON");
    for value in ["\"20\"", "20.0", "-1", "null", "true"] {
        let malformed = canonical.replace(
            "\"timeout_minutes\":5",
            &format!("\"timeout_minutes\":{value}"),
        );
        assert_ne!(malformed, canonical);
        assert!(
            serde_json::from_str::<RustBinaryReleaseProducer>(&malformed).is_err(),
            "{value}"
        );
    }
    let duplicate = canonical.replace(
        "\"timeout_minutes\":5",
        "\"timeout_minutes\":20,\"timeout_minutes\":5",
    );
    assert!(serde_json::from_str::<RustBinaryReleaseProducer>(&duplicate).is_err());
}
