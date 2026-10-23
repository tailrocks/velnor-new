use super::{
    DEFAULT_COOLDOWN_DAYS, MAX_COOLDOWN_DAYS, MIN_COOLDOWN_DAYS, check_policy_header,
    policy_mise_version, toolchain_specs,
};
use crate::context::FreshnessContext;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn verify_local_toolchain_specs_come_from_policy_when_local_mise_pins_differ() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let specs = toolchain_specs(&root);
    assert!(specs.is_ok(), "toolchain specs: {specs:?}");
    let specs = specs.unwrap_or_default();
    assert_eq!(specs.len(), 3);
    assert!(specs.contains(&"rust@1.99.1".to_owned()));
    assert!(specs.contains(&"mr-boxington@1.27.0".to_owned()));
    assert!(specs.contains(&"aqua:nextest-rs/nextest/cargo-nextest@0.9.149".to_owned()));
    assert_eq!(policy_mise_version(&root), Ok("2026.10.18".to_owned()));
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
        "[tools]\nrust = \"1.99.0\"\nmr-boxington = \"1.22.0\"\nnextest = \"0.9.149\"\n",
    );
    assert!(policy.is_ok(), "fixture policy: {policy:?}");
    assert_eq!(
        toolchain_specs(&fixture_root),
        Ok(vec![
            "rust@1.99.0".to_owned(),
            "mr-boxington@1.22.0".to_owned(),
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.149".to_owned(),
        ])
    );
    let cleanup = fs::remove_dir_all(fixture_root);
    assert!(cleanup.is_ok(), "fixture cleanup: {cleanup:?}");
}

fn eval_policy_header(extra_header: &str) -> FreshnessContext {
    let toml_text = format!(
        "schema = 1\nchannel = \"stable\"\nregistry = \"https://github.com/rust-lang/crates.io-index\"\ncheck_interval_hours = 24\nmax_exception_days = 14\n{extra_header}"
    );
    let mut ctx = FreshnessContext::new(PathBuf::from("."), false, false);
    ctx.policy = toml::from_str(&toml_text).ok();
    check_policy_header(&mut ctx);
    ctx
}

#[test]
fn policy_header_default_cooldown_days_defaults_to_three_and_bounds_one_to_ninety() {
    assert_eq!(DEFAULT_COOLDOWN_DAYS, 3);
    assert_eq!(MIN_COOLDOWN_DAYS, 1);
    assert_eq!(MAX_COOLDOWN_DAYS, 90);

    let omitted = eval_policy_header("");
    assert!(omitted.failures.is_empty(), "{:?}", omitted.failures);
    assert_eq!(omitted.cooldown_days, 3);

    for valid in [1, 3, 7, 30, 90] {
        let ctx = eval_policy_header(&format!("default_cooldown_days = {valid}\n"));
        assert!(ctx.failures.is_empty(), "valid={valid}: {:?}", ctx.failures);
        assert_eq!(ctx.cooldown_days, valid);
    }

    for invalid in ["0", "-1", "91", "\"3\""] {
        let ctx = eval_policy_header(&format!("default_cooldown_days = {invalid}\n"));
        assert!(
            ctx.failures
                .iter()
                .any(|failure| failure.contains("policy-header default_cooldown_days:")),
            "invalid={invalid} should fail: {:?}",
            ctx.failures
        );
    }
}
