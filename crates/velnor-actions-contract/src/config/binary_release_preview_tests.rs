use super::*;
use crate::config::{CheckSystemTool, CheckSystemToolKind};

fn enabled_preview() -> RustBinaryPreviewConfig {
    RustBinaryPreviewConfig {
        enabled: true,
        version_env: "JACKIN_VERSION_OVERRIDE".to_owned(),
        producer: RustBinaryPreviewProducer {
            task: "release:preview-package".to_owned(),
            tools: vec!["rust".to_owned()],
            inputs: vec!["Cargo.lock".to_owned(), "mise.toml".to_owned()],
            ..RustBinaryPreviewProducer::default()
        },
        products: vec![RustBinaryPreviewProduct {
            id: "jackin".to_owned(),
            package: "jackin".to_owned(),
            bins: vec!["jackin".to_owned(), "jackin-role".to_owned()],
            targets: vec![
                "aarch64-apple-darwin".to_owned(),
                "x86_64-unknown-linux-gnu".to_owned(),
            ],
            archive: RustBinaryPreviewArchive::TarGz,
        }],
        companions: vec![RustBinaryPreviewCompanion {
            id: "capsule-manifest".to_owned(),
            path: "capsule/manifest.json".to_owned(),
            asset_name: "capsule-manifest.json".to_owned(),
        }],
        ..RustBinaryPreviewConfig::default()
    }
}

#[test]
fn preview_defaults_independently_disabled_with_mandatory_evidence() {
    let preview = RustBinaryPreviewConfig::default();
    assert!(!preview.enabled);
    assert_eq!(preview.version_env, "");
    assert_eq!(
        preview.evidence.profile,
        RustBinaryPreviewEvidenceProfile::SignedSbomV1
    );
    assert!(preview.validate("config.toml").is_ok());
}

#[test]
fn enabled_preview_accepts_explicit_product_matrix_and_companion_assets() {
    let preview = enabled_preview();
    assert!(preview.validate("config.toml").is_ok());
    assert_eq!(preview.version_env, "JACKIN_VERSION_OVERRIDE");
    assert_eq!(
        preview.versioning,
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
    let json = r#"{"enabled":true,"versioning":"package-preview-commit-count-sha7","version_env":"JACKIN_VERSION_OVERRIDE","environment":"preview-publish","producer":{"task":"release:preview-package","source":{"mise_config":"mise.toml","working_directory":"."},"output_directory":"dist/preview","manifest":"dist/preview/package-output.json","tools":["rust"],"inputs":["Cargo.lock","mise.toml"],"runner":"linux-x64","timeout_minutes":120,"build_jobs":2},"products":[{"id":"jackin","package":"jackin","bins":["jackin"],"targets":["aarch64-apple-darwin"],"archive":"tar.gz"}],"companions":[],"evidence":{"profile":"signed-sbom-v1","checksums":false}}"#;
    assert!(serde_json::from_str::<RustBinaryPreviewConfig>(json).is_err());
    let valid = json.replace(",\"checksums\":false", "");
    let config = serde_json::from_str::<RustBinaryPreviewConfig>(&valid).expect("valid preview");
    assert!(config.validate("config.toml").is_ok());
    assert!(
        serde_json::from_str::<RustBinaryPreviewConfig>(
            &valid.replace(",\"evidence\":{\"profile\":\"signed-sbom-v1\"}", "")
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<RustBinaryPreviewConfig>(
            &valid.replace("\"profile\":\"signed-sbom-v1\"", "")
        )
        .is_err()
    );
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
    preview.environment = "consumer-binary-release".to_owned();
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
        system_tool(CheckSystemToolKind::Swift, "6.4", "swiftlang-6.4.0.34.1"),
        system_tool(CheckSystemToolKind::Xcode, "26.0", "16A242d"),
    ];
    assert!(preview.validate("config.toml").is_ok());

    let mut linux = enabled_preview();
    linux.producer.system_tools = preview.producer.system_tools.clone();
    assert!(linux.validate("config.toml").is_err());

    let mut duplicate = preview.clone();
    duplicate.producer.system_tools = vec![
        system_tool(CheckSystemToolKind::Swift, "6.4", "swiftlang-6.4.0.34.1"),
        system_tool(CheckSystemToolKind::Swift, "6.4", "swiftlang-6.4.0.34.1"),
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
        RustBinaryPreviewProduct {
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
            archive: RustBinaryPreviewArchive::TarGz,
        },
        RustBinaryPreviewProduct {
            id: "jackin-capsule".to_owned(),
            package: "jackin-capsule".to_owned(),
            bins: vec!["jackin-capsule".to_owned()],
            targets: vec![
                "aarch64-unknown-linux-gnu".to_owned(),
                "x86_64-unknown-linux-gnu".to_owned(),
            ],
            archive: RustBinaryPreviewArchive::TarGz,
        },
    ];
    preview.companions = vec![RustBinaryPreviewCompanion {
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
        .map(|index| RustBinaryPreviewCompanion {
            id: format!("asset-{index}"),
            path: format!("assets/{index}.json"),
            asset_name: format!("asset-{index}.json"),
        })
        .collect();
    assert!(preview.validate("config.toml").is_err());
}
