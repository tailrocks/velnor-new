//! Tool catalog pin cases.
use velnor_actions_mise::catalog::{MbxProvisioning, NEXTEST_VERSION};
use velnor_actions_mise::{
    GH_VERSION, JACTIONLINT_VERSION, MISE_VERSION, MR_BOXINGTON_VERSION, MiseError,
    OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64, OPENTOFU_SHA256_LINUX_AMD64,
    OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION, PinnedTool, RUST_TARGET_TRIPLE, RUST_VERSION,
    SHELLCHECK_VERSION, ToolCatalog, validate_exact_version,
};

#[test]
fn pinned_catalog_matches_qualified_versions() {
    assert_eq!(MISE_VERSION, "2026.10.18");
    let catalog = ToolCatalog::pinned();
    assert_eq!(catalog.version(PinnedTool::Rust), "1.99.1");
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.27.0");
    assert_eq!(catalog.version(PinnedTool::Gh), "2.103.0");
    assert_eq!(catalog.version(PinnedTool::Jactionlint), "2.3.0");
    assert_eq!(catalog.version(PinnedTool::Shellcheck), "0.11.0");
    assert_eq!(catalog.version(PinnedTool::Nextest), "0.9.149");
    assert_eq!(catalog.version(PinnedTool::Opentofu), "1.13.2");
    assert_eq!(RUST_VERSION, "1.99.1");
    assert_eq!(MR_BOXINGTON_VERSION, "1.27.0");
    assert_eq!(GH_VERSION, "2.103.0");
    assert_eq!(JACTIONLINT_VERSION, "2.3.0");
    assert_eq!(SHELLCHECK_VERSION, "0.11.0");
    assert_eq!(NEXTEST_VERSION, "0.9.149");
    assert_eq!(OPENTOFU_VERSION, "1.13.2");
}

#[test]
fn tool_specs_use_registry_names() {
    let catalog = ToolCatalog::pinned();
    assert_eq!(catalog.tool_spec(PinnedTool::Rust), "rust@1.99.1");
    assert_eq!(
        catalog.tool_spec(PinnedTool::MrBoxington),
        "mr-boxington@1.27.0"
    );
    assert_eq!(catalog.tool_spec(PinnedTool::Gh), "gh@2.103.0");
    assert_eq!(
        catalog.tool_spec(PinnedTool::Jactionlint),
        "jactionlint@2.3.0"
    );
    assert_eq!(
        catalog.tool_spec(PinnedTool::Shellcheck),
        "shellcheck@0.11.0"
    );
    assert_eq!(
        catalog.tool_spec(PinnedTool::Nextest),
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.149"
    );
    assert_eq!(catalog.tool_spec(PinnedTool::Opentofu), "opentofu@1.13.2");
    assert_eq!(
        catalog.tool_specs(&[PinnedTool::Rust, PinnedTool::MrBoxington]),
        vec!["rust@1.99.1".to_owned(), "mr-boxington@1.27.0".to_owned()]
    );
}

#[test]
fn tool_names_roundtrip_and_reject_aliases() {
    assert_eq!(PinnedTool::ALL.len(), 10);
    for tool in PinnedTool::ALL {
        assert_eq!(PinnedTool::from_tool_name(tool.tool_name()), Ok(tool));
    }
    for name in [
        "mbx",
        "mr_boxington",
        "cargo",
        "rustc",
        "cargo-nextest",
        "node",
        "",
    ] {
        assert!(
            matches!(
                PinnedTool::from_tool_name(name),
                Err(MiseError::UnknownTool { .. })
            ),
            "name must be rejected: {name}"
        );
    }
}

#[test]
fn exact_version_validation_accepts_only_pins() {
    for version in ["1.99.1", "2026.10.13", "0.11.0", "10.20.30"] {
        assert!(
            validate_exact_version("rust", version).is_ok(),
            "version must be accepted: {version}"
        );
    }
    for version in [
        "v1.9.0",
        "latest",
        "1.9",
        "1.9.0-beta",
        "",
        "1.9.x",
        "1..3",
        "1.2.3.4",
    ] {
        assert!(
            matches!(
                validate_exact_version("rust", version),
                Err(MiseError::InvalidToolVersion { .. })
            ),
            "version must be rejected: {version}"
        );
    }
}

#[test]
fn tool_file_values_never_become_pins() {
    for tool in PinnedTool::ALL {
        let catalog = ToolCatalog::pinned();
        let version = catalog.version(tool);
        assert!(
            validate_exact_version(tool.tool_name(), version).is_ok(),
            "pinned version is exact: {version}"
        );
    }
    for loose in [
        "latest", "stable", "system", "1.98", "v1.99.1", ">=1.98", "",
    ] {
        assert!(
            matches!(
                validate_exact_version("rust", loose),
                Err(MiseError::InvalidToolVersion { .. })
            ),
            "tool-file-style selector must fail: {loose}"
        );
    }
}

#[test]
fn catalog_new_validates_every_slot() {
    let catalog = ToolCatalog::new(
        "1.99.1", "1.26.0", "2.103.0", "2.3.0", "0.11.0", "0.9.149", "1.13.2",
    );
    assert!(catalog.is_ok());
    assert!(matches!(
        ToolCatalog::new(
            "latest", "1.26.0", "2.103.0", "2.3.0", "0.11.0", "0.9.149", "1.13.2"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
    assert!(matches!(
        ToolCatalog::new(
            "1.99.1", "1.26.0", "2.103.0", "v2.3.0", "0.11.0", "0.9.149", "1.13.2"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
    assert!(matches!(
        ToolCatalog::new(
            "1.99.1", "1.26.0", "2.103.0", "2.3.0", "0.11.0", "latest", "1.13.2"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
    assert!(matches!(
        ToolCatalog::new(
            "1.99.1", "1.26.0", "2.103.0", "2.3.0", "0.11.0", "0.9.149", "latest"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
}

#[test]
fn rust_toolchain_name_pins_version_and_target() {
    assert_eq!(RUST_TARGET_TRIPLE, "x86_64-unknown-linux-gnu");
    assert!(velnor_actions_contract::targets::is_supported_target(
        RUST_TARGET_TRIPLE
    ));
    assert_eq!(
        ToolCatalog::pinned().rust_toolchain_name(),
        "1.99.1-x86_64-unknown-linux-gnu"
    );
}

#[test]
fn action_mbx_reconcile_matches_pin_only() {
    let catalog = ToolCatalog::pinned();
    assert!(catalog.reconcile_action_mbx(MR_BOXINGTON_VERSION).is_ok());
    assert!(matches!(
        catalog.reconcile_action_mbx("1.22.0"),
        Err(MiseError::InvalidToolVersion { tool, version })
            if tool == "mr-boxington" && version == "1.22.0"
    ));
    for loose in ["latest", "v1.26.0", "1.19", ""] {
        assert!(
            matches!(
                catalog.reconcile_action_mbx(loose),
                Err(MiseError::InvalidToolVersion { .. })
            ),
            "{loose} must fail closed"
        );
    }
    let moved = ToolCatalog::new(
        "1.99.1", "1.22.0", "2.103.0", "2.3.0", "0.11.0", "0.9.149", "1.13.1",
    )
    .expect("exact catalog");
    assert!(moved.reconcile_action_mbx("1.22.0").is_ok());
    assert!(moved.reconcile_action_mbx(MR_BOXINGTON_VERSION).is_err());
}

/// Qualified `tofu_1.13.2_<platform>.tar.gz` sha256 digests (T15).
const OPENTOFU_LINUX_AMD64: &str =
    "ac05d9952662c7db28dd57e93f0a68018837bfdf9342384ec0987977c16a9645";
const OPENTOFU_LINUX_ARM64: &str =
    "88e2d9e3f94f97b76ab2faf55ade87578f56aed19c5e45fa139d70e994366d1f";
const OPENTOFU_DARWIN_AMD64: &str =
    "dc6e4eb1d3ed6550c2ef052596f933dbcd8c806957665ea83ae7537147cb3074";
const OPENTOFU_DARWIN_ARM64: &str =
    "0d1cdc85f38992055b48e7fb161304a2ddfe2e5ee0ff68ffec9d0349c8bd1b59";

#[test]
fn opentofu_catalog_digest_binds_qualified_artifact() {
    assert_eq!(OPENTOFU_SHA256_LINUX_AMD64, OPENTOFU_LINUX_AMD64);
    let catalog = ToolCatalog::pinned();
    let identity = catalog.tool_identity(PinnedTool::Opentofu);
    assert_eq!(identity.digest, OPENTOFU_LINUX_AMD64);
    assert_eq!(identity.version, "1.13.2");
    assert_eq!(
        identity.source,
        "https://github.com/opentofu/opentofu/releases/tag/v1.13.2"
    );
    assert!(identity.validate("catalog").is_ok());
    let spec = catalog.tool_spec(PinnedTool::Opentofu);
    assert_eq!(spec, "opentofu@1.13.2");
    assert!(!spec.contains(':'), "mise selector stays bare shorthand");
}

#[test]
fn opentofu_platform_digests_are_distinct_qualified_hex() {
    assert_eq!(OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_LINUX_ARM64);
    assert_eq!(OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_DARWIN_AMD64);
    assert_eq!(OPENTOFU_SHA256_DARWIN_ARM64, OPENTOFU_DARWIN_ARM64);
    let digests = [
        OPENTOFU_SHA256_LINUX_AMD64,
        OPENTOFU_SHA256_LINUX_ARM64,
        OPENTOFU_SHA256_DARWIN_AMD64,
        OPENTOFU_SHA256_DARWIN_ARM64,
    ];
    for digest in digests {
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "digest must be lowercase hex"
        );
        assert!(
            digest.bytes().any(|byte| byte != b'0'),
            "digest must never be the all-zero placeholder"
        );
    }
    let mut sorted = digests.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), digests.len(), "per-platform digests differ");
}

#[test]
fn opentofu_digest_follows_qualified_version_only() {
    let pinned = ToolCatalog::pinned();
    assert!(
        pinned
            .tool_identity(PinnedTool::Opentofu)
            .validate("catalog")
            .is_ok()
    );
    let moved = ToolCatalog::new(
        "1.99.1", "1.26.0", "2.103.0", "2.3.0", "0.11.0", "0.9.149", "1.13.0",
    )
    .expect("exact catalog");
    let err = moved
        .tool_identity(PinnedTool::Opentofu)
        .validate("catalog")
        .expect_err("unqualified version never carries the qualified digest");
    assert!(err.to_string().contains("placeholder_digest"), "{err}");
}

#[test]
fn mbx_provisioning_modes_validated() {
    assert_eq!(
        MbxProvisioning::preinstalled("/opt/mbx/bin/mbx").expect("absolute path"),
        MbxProvisioning::PreinstalledTool {
            tool_path: "/opt/mbx/bin/mbx".to_owned(),
        }
    );
    for bad in ["", "relative/mbx", "mbx", "C:\\mbx\\mbx", "/tmp/has\0nul"] {
        assert!(
            matches!(
                MbxProvisioning::preinstalled(bad),
                Err(MiseError::InvalidStepInput { field, .. }) if field == "tool_path"
            ),
            "{bad:?} must fail closed"
        );
    }
    assert_eq!(
        MbxProvisioning::CatalogInstall,
        MbxProvisioning::CatalogInstall
    );
    assert_eq!(
        MbxProvisioning::ActionExactVersion,
        MbxProvisioning::ActionExactVersion
    );
}
