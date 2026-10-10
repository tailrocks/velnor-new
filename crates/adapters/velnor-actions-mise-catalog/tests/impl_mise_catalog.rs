//! Tool catalog pin cases.
use velnor_actions_mise_catalog::catalog::{MbxProvisioning, NEXTEST_VERSION};
use velnor_actions_mise_catalog::{
    ACTIONLINT_VERSION, GH_VERSION, MISE_VERSION, MR_BOXINGTON_VERSION,
    OPENTOFU_SHA256_DARWIN_AMD64, OPENTOFU_SHA256_DARWIN_ARM64, OPENTOFU_SHA256_LINUX_AMD64,
    OPENTOFU_SHA256_LINUX_ARM64, OPENTOFU_VERSION, PinnedTool, RUST_TARGET_TRIPLE, RUST_VERSION,
    SHELLCHECK_VERSION, ToolCatalog, ZIZMOR_VERSION, validate_exact_version,
};
use velnor_actions_mise_core::MiseError;

#[test]
fn pinned_catalog_matches_qualified_versions() {
    assert_eq!(MISE_VERSION, "2026.9.18");
    let catalog = ToolCatalog::pinned();
    assert_eq!(catalog.version(PinnedTool::Rust), "1.98.1");
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.21.1");
    assert_eq!(catalog.version(PinnedTool::Gh), "2.102.0");
    assert_eq!(catalog.version(PinnedTool::Actionlint), "1.7.12");
    assert_eq!(catalog.version(PinnedTool::Shellcheck), "0.11.0");
    assert_eq!(catalog.version(PinnedTool::Zizmor), "1.30.1");
    assert_eq!(catalog.version(PinnedTool::Nextest), "0.9.148");
    assert_eq!(catalog.version(PinnedTool::Opentofu), "1.13.1");
    assert_eq!(RUST_VERSION, "1.98.1");
    assert_eq!(MR_BOXINGTON_VERSION, "1.21.1");
    assert_eq!(GH_VERSION, "2.102.0");
    assert_eq!(ACTIONLINT_VERSION, "1.7.12");
    assert_eq!(SHELLCHECK_VERSION, "0.11.0");
    assert_eq!(ZIZMOR_VERSION, "1.30.1");
    assert_eq!(NEXTEST_VERSION, "0.9.148");
    assert_eq!(OPENTOFU_VERSION, "1.13.1");
}

#[test]
fn tool_specs_use_registry_names() {
    let catalog = ToolCatalog::pinned();
    assert_eq!(catalog.tool_spec(PinnedTool::Rust), "rust@1.98.1");
    assert_eq!(
        catalog.tool_spec(PinnedTool::MrBoxington),
        "mr-boxington@1.21.1"
    );
    assert_eq!(catalog.tool_spec(PinnedTool::Gh), "gh@2.102.0");
    assert_eq!(
        catalog.tool_spec(PinnedTool::Actionlint),
        "actionlint@1.7.12"
    );
    assert_eq!(
        catalog.tool_spec(PinnedTool::Shellcheck),
        "shellcheck@0.11.0"
    );
    assert_eq!(catalog.tool_spec(PinnedTool::Zizmor), "zizmor@1.30.1");
    assert_eq!(
        catalog.tool_spec(PinnedTool::Nextest),
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.148"
    );
    assert_eq!(catalog.tool_spec(PinnedTool::Opentofu), "opentofu@1.13.1");
    assert_eq!(
        catalog.tool_specs(&[PinnedTool::Rust, PinnedTool::MrBoxington]),
        vec!["rust@1.98.1".to_owned(), "mr-boxington@1.21.1".to_owned()]
    );
}

#[test]
fn tool_names_roundtrip_and_reject_aliases() {
    assert_eq!(PinnedTool::ALL.len(), 9);
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
    for version in ["1.98.1", "2026.9.18", "0.11.0", "10.20.30"] {
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
        "latest", "stable", "system", "1.98", "v1.98.1", ">=1.98", "",
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
        "1.98.1", "1.21.1", "2.102.0", "1.7.12", "0.11.0", "1.30.1", "0.9.148", "1.13.1",
    );
    assert!(catalog.is_ok());
    assert!(matches!(
        ToolCatalog::new(
            "latest", "1.21.1", "2.102.0", "1.7.12", "0.11.0", "1.30.1", "0.9.148", "1.13.1"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
    assert!(matches!(
        ToolCatalog::new(
            "1.98.1", "1.21.1", "2.102.0", "1.7.12", "0.11.0", "v1.30.1", "0.9.148", "1.13.1"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
    assert!(matches!(
        ToolCatalog::new(
            "1.98.1", "1.21.1", "2.102.0", "1.7.12", "0.11.0", "1.30.1", "latest", "1.13.1"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
    assert!(matches!(
        ToolCatalog::new(
            "1.98.1", "1.21.1", "2.102.0", "1.7.12", "0.11.0", "1.30.1", "0.9.148", "latest"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
}

#[test]
fn rust_toolchain_name_pins_version_and_target() {
    assert_eq!(RUST_TARGET_TRIPLE, "x86_64-unknown-linux-gnu");
    assert!(velnor_actions_contract_release::targets::is_supported_target(RUST_TARGET_TRIPLE));
    assert_eq!(
        ToolCatalog::pinned().rust_toolchain_name(),
        "1.98.1-x86_64-unknown-linux-gnu"
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
    for loose in ["latest", "v1.21.1", "1.19", ""] {
        assert!(
            matches!(
                catalog.reconcile_action_mbx(loose),
                Err(MiseError::InvalidToolVersion { .. })
            ),
            "{loose} must fail closed"
        );
    }
    let moved = ToolCatalog::new(
        "1.98.1", "1.22.0", "2.102.0", "1.7.12", "0.11.0", "1.30.1", "0.9.148", "1.13.1",
    )
    .expect("exact catalog");
    assert!(moved.reconcile_action_mbx("1.22.0").is_ok());
    assert!(moved.reconcile_action_mbx(MR_BOXINGTON_VERSION).is_err());
}

/// Qualified `tofu_1.13.1_<platform>.tar.gz` sha256 digests (T15).
const OPENTOFU_LINUX_AMD64: &str =
    "378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69";
const OPENTOFU_LINUX_ARM64: &str =
    "9c1ef375aa1852db0b2888aa921b640c71f8140d4682aa4fec99378a64fa7dc3";
const OPENTOFU_DARWIN_AMD64: &str =
    "a73720443ba38712d7d96dc1e857add02c15a790919c653ad07492e9952f8c27";
const OPENTOFU_DARWIN_ARM64: &str =
    "be78f659f04ef06a9dbd9b3934d46af95d787a3aa38396d459dea395261816a9";

#[test]
fn opentofu_catalog_digest_binds_qualified_artifact() {
    assert_eq!(OPENTOFU_SHA256_LINUX_AMD64, OPENTOFU_LINUX_AMD64);
    let catalog = ToolCatalog::pinned();
    let identity = catalog.tool_identity(PinnedTool::Opentofu);
    assert_eq!(identity.digest, OPENTOFU_LINUX_AMD64);
    assert_eq!(identity.version, "1.13.1");
    assert_eq!(
        identity.source,
        "https://github.com/opentofu/opentofu/releases/tag/v1.13.1"
    );
    assert!(identity.validate("catalog").is_ok());
    let spec = catalog.tool_spec(PinnedTool::Opentofu);
    assert_eq!(spec, "opentofu@1.13.1");
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
        "1.98.1", "1.21.1", "2.102.0", "1.7.12", "0.11.0", "1.30.1", "0.9.148", "1.13.0",
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
