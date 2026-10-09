use super::{policy_mise_version, toolchain_specs};
use std::fs;
use std::path::Path;

#[test]
fn verify_local_toolchain_specs_come_from_policy_when_local_mise_pins_differ() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let specs = toolchain_specs(&root);
    assert!(specs.is_ok(), "toolchain specs: {specs:?}");
    let specs = specs.unwrap_or_default();
    assert_eq!(specs.len(), 3);
    assert!(specs.contains(&"rust@1.98.1".to_owned()));
    assert!(specs.contains(&"mr-boxington@1.21.1".to_owned()));
    assert!(specs.contains(&"aqua:nextest-rs/nextest/cargo-nextest@0.9.148".to_owned()));
    assert_eq!(policy_mise_version(&root), Ok("2026.10.6".to_owned()));
    let check_freshness = fs::read_to_string(root.join("scripts/check-freshness.sh"));
    assert!(
        check_freshness.is_ok(),
        "check-freshness script: {check_freshness:?}"
    );
    let check_freshness = check_freshness.unwrap_or_default();
    assert!(check_freshness.contains("awk -v key=mr-boxington"));
    assert!(check_freshness.contains("\"$ROOT/.velnor/version-policy.toml\""));
    assert!(
        check_freshness.contains("mise exec \"rust@$RUST_POLICY\" \"mr-boxington@$MBX_POLICY\" --")
    );

    let fixture_root = std::env::temp_dir().join(format!(
        "velnor-local-toolchain-policy-{}",
        std::process::id()
    ));
    let fixture_policy = fixture_root.join(".velnor/version-policy.toml");
    let create_result = fs::create_dir_all(fixture_root.join(".velnor"));
    assert!(create_result.is_ok(), "fixture setup: {create_result:?}");
    let local_mise = fs::write(
        fixture_root.join("mise.toml"),
        "[tools]\nrust = \"1.98.1\"\nmr-boxington = \"1.21.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.146\"\n",
    );
    assert!(local_mise.is_ok(), "fixture mise.toml: {local_mise:?}");
    let policy = fs::write(
        fixture_policy,
        "[tools]\nrust = \"1.99.0\"\nmr-boxington = \"1.22.0\"\nnextest = \"0.9.148\"\n",
    );
    assert!(policy.is_ok(), "fixture policy: {policy:?}");
    assert_eq!(
        toolchain_specs(&fixture_root),
        Ok(vec![
            "rust@1.99.0".to_owned(),
            "mr-boxington@1.22.0".to_owned(),
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.148".to_owned(),
        ])
    );
    let cleanup = fs::remove_dir_all(fixture_root);
    assert!(cleanup.is_ok(), "fixture cleanup: {cleanup:?}");
}
