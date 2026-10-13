//! Step-env provisioning and cache-key derivation.

use std::collections::BTreeMap;

use velnor_actions_mise::ToolCatalog;
use velnor_actions_orchestrator_provisioning::matrix_step::task_step_env;
use velnor_actions_orchestrator_provisioning::source_cache::sources_cache_key;

#[test]
fn rust_step_env_carries_isolation_and_homes() {
    let env = task_step_env(&ToolCatalog::pinned(), &BTreeMap::new(), true).expect("env");
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_AUTO_INSTALL",
    ] {
        assert!(env.contains_key(key), "missing {key}: {env:?}");
    }
    assert!(env.contains_key("MISE_RUSTUP_HOME"), "{env:?}");
    assert!(env.contains_key("MISE_CARGO_HOME"), "{env:?}");
}

#[test]
fn non_rust_step_env_skips_toolchain_homes() {
    let env = task_step_env(&ToolCatalog::pinned(), &BTreeMap::new(), false).expect("env");
    assert!(env.contains_key("MISE_NO_CONFIG"), "{env:?}");
    assert!(!env.contains_key("MISE_RUSTUP_HOME"), "{env:?}");
    assert!(!env.contains_key("MISE_CARGO_HOME"), "{env:?}");
}

#[test]
fn reserved_extra_key_fails_closed() {
    let extra = BTreeMap::from([("MISE_NO_CONFIG".to_owned(), "0".to_owned())]);
    let err = task_step_env(&ToolCatalog::pinned(), &extra, true).expect_err("reserved");
    assert!(err.to_string().contains("reserved_step_env"), "{err}");
}

#[test]
fn extra_keys_pass_through() {
    let extra = BTreeMap::from([("VELNOR_CUSTOM".to_owned(), "1".to_owned())]);
    let env = task_step_env(&ToolCatalog::pinned(), &extra, true).expect("env");
    assert_eq!(env.get("VELNOR_CUSTOM").map(String::as_str), Some("1"));
}

#[test]
fn cache_key_is_deterministic() {
    let roots = vec!["crates/a".to_owned(), "crates/b".to_owned()];
    let first = sources_cache_key("x86_64-unknown-linux-gnu", "1.98.1", &roots).expect("key");
    let second = sources_cache_key("x86_64-unknown-linux-gnu", "1.98.1", &roots).expect("key");
    assert!(!first.is_empty());
    assert_eq!(first, second);
}

#[test]
fn cache_key_separates_targets_and_toolchains() {
    let roots = vec!["crates/a".to_owned()];
    let linux = sources_cache_key("x86_64-unknown-linux-gnu", "1.98.1", &roots).expect("key");
    let macos = sources_cache_key("aarch64-apple-darwin", "1.98.1", &roots).expect("key");
    let older = sources_cache_key("x86_64-unknown-linux-gnu", "1.90.0", &roots).expect("key");
    assert_ne!(linux, macos);
    assert_ne!(linux, older);
}
