//! Tool catalog pin cases.
use velnor_actions_mise::catalog::NEXTEST_VERSION;
use velnor_actions_mise::{
    ACTIONLINT_VERSION, GH_VERSION, MISE_VERSION, MR_BOXINGTON_VERSION, MiseError, PinnedTool,
    RUST_VERSION, SHELLCHECK_VERSION, ToolCatalog, ZIZMOR_VERSION, validate_exact_version,
};

#[test]
fn pinned_catalog_matches_qualified_versions() {
    assert_eq!(MISE_VERSION, "2026.9.16");
    let catalog = ToolCatalog::pinned();
    assert_eq!(catalog.version(PinnedTool::Rust), "1.98.1");
    assert_eq!(catalog.version(PinnedTool::MrBoxington), "1.19.0");
    assert_eq!(catalog.version(PinnedTool::Gh), "2.101.0");
    assert_eq!(catalog.version(PinnedTool::Actionlint), "1.7.12");
    assert_eq!(catalog.version(PinnedTool::Shellcheck), "0.11.0");
    assert_eq!(catalog.version(PinnedTool::Zizmor), "1.30.1");
    assert_eq!(catalog.version(PinnedTool::Nextest), "0.9.146");
    assert_eq!(RUST_VERSION, "1.98.1");
    assert_eq!(MR_BOXINGTON_VERSION, "1.19.0");
    assert_eq!(GH_VERSION, "2.101.0");
    assert_eq!(ACTIONLINT_VERSION, "1.7.12");
    assert_eq!(SHELLCHECK_VERSION, "0.11.0");
    assert_eq!(ZIZMOR_VERSION, "1.30.1");
    assert_eq!(NEXTEST_VERSION, "0.9.146");
}

#[test]
fn tool_specs_use_registry_names() {
    let catalog = ToolCatalog::pinned();
    assert_eq!(catalog.tool_spec(PinnedTool::Rust), "rust@1.98.1");
    assert_eq!(
        catalog.tool_spec(PinnedTool::MrBoxington),
        "mr-boxington@1.19.0"
    );
    assert_eq!(catalog.tool_spec(PinnedTool::Gh), "gh@2.101.0");
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
        "aqua:nextest-rs/nextest/cargo-nextest@0.9.146"
    );
    assert_eq!(
        catalog.tool_specs(&[PinnedTool::Rust, PinnedTool::MrBoxington]),
        vec!["rust@1.98.1".to_owned(), "mr-boxington@1.19.0".to_owned()]
    );
}

#[test]
fn tool_names_roundtrip_and_reject_aliases() {
    assert_eq!(PinnedTool::ALL.len(), 7);
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
    for version in ["1.98.1", "2026.9.16", "0.11.0", "10.20.30"] {
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
        "1.98.1", "1.19.0", "2.101.0", "1.7.12", "0.11.0", "1.30.1", "0.9.146",
    );
    assert!(catalog.is_ok());
    assert!(matches!(
        ToolCatalog::new(
            "latest", "1.19.0", "2.101.0", "1.7.12", "0.11.0", "1.30.1", "0.9.146"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
    assert!(matches!(
        ToolCatalog::new(
            "1.98.1", "1.19.0", "2.101.0", "1.7.12", "0.11.0", "v1.30.1", "0.9.146"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
    assert!(matches!(
        ToolCatalog::new(
            "1.98.1", "1.19.0", "2.101.0", "1.7.12", "0.11.0", "1.30.1", "latest"
        ),
        Err(MiseError::InvalidToolVersion { .. })
    ));
}
