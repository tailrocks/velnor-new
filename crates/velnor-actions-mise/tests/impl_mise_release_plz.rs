//! Release-plz preparation pin and repository evidence.
use velnor_actions_mise::catalog::RELEASE_PLZ_VERSION;
use velnor_actions_mise::catalog::lock::verify_version_policy;
use velnor_actions_mise::catalog::release_plz::RELEASE_PLZ_CKSUM;
use velnor_actions_mise::{PinnedTool, ToolCatalog, validate_exact_version};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

#[test]
fn release_plz_pin_is_exact() {
    assert_eq!(RELEASE_PLZ_VERSION, "0.3.169");
    assert_eq!(pinned().version(PinnedTool::ReleasePlz), "0.3.169");
    assert_eq!(
        pinned()
            .tool_spec(PinnedTool::ReleasePlz)
            .expect("release-plz tool spec"),
        "release-plz@0.3.169"
    );
    assert_eq!(PinnedTool::ReleasePlz.tool_name(), "release-plz");
    assert_eq!(
        PinnedTool::from_tool_name("release-plz"),
        Ok(PinnedTool::ReleasePlz)
    );
    assert!(validate_exact_version("release-plz", RELEASE_PLZ_VERSION).is_ok());
    let err = pinned()
        .tool_identity(PinnedTool::ReleasePlz)
        .validate("catalog")
        .expect_err("placeholder digests never validate as trusted");
    assert!(err.to_string().contains("placeholder_digest"), "{err}");
}

#[test]
fn release_plz_cksum_is_full_sha256() {
    assert_eq!(
        RELEASE_PLZ_CKSUM,
        "2f7a1b17465db464a28627bae7832ff7eb9b5f29b4fe89048b4dde8da1f567e5"
    );
    assert_eq!(RELEASE_PLZ_CKSUM.len(), 64);
    assert!(
        RELEASE_PLZ_CKSUM
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()),
        "cksum must be full hex, never truncated"
    );
}

#[test]
fn repo_policy_mirror_covers_release_plz() {
    let path = format!(
        "{}/../../.velnor/version-policy.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).expect("repo version-policy exists");
    assert!(
        text.contains("release-plz = \"0.3.169\""),
        "policy pins release-plz"
    );
    verify_version_policy(&text, &pinned()).expect("policy mirrors catalog");
}

#[test]
fn freshness_inventory_mirrors_release_plz() {
    let path = format!(
        "{}/../../.velnor/freshness-inventory.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).expect("freshness inventory exists");
    for needle in [
        "\"name\": \"release-plz\"",
        "\"pinned\": \"0.3.169\"",
        "\"qualified\": \"0.3.169\"",
        "\"source\": \"https://crates.io/api/v1/crates/release-plz\"",
    ] {
        assert!(
            text.contains(needle),
            "inventory mirrors release-plz: {needle}"
        );
    }
}
