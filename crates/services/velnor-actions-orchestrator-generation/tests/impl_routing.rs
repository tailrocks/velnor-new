//! Dispatch-mode parsing plus schema 2 config migration.

use velnor_actions_contract_config::ExecutionMode;
use velnor_actions_orchestrator_generation::routing::{migrate_config, parse_dispatch_mode};

#[test]
fn dispatch_modes_parse() {
    assert_eq!(
        parse_dispatch_mode("hosted").expect("hosted"),
        ExecutionMode::Hosted
    );
    assert_eq!(
        parse_dispatch_mode("scale-set").expect("scale-set"),
        ExecutionMode::ScaleSet
    );
    assert_eq!(
        parse_dispatch_mode("both").expect("both"),
        ExecutionMode::Both
    );
}

#[test]
fn dispatch_mode_rejects_unknown_spellings() {
    assert!(parse_dispatch_mode("never").is_err());
    assert!(parse_dispatch_mode("").is_err());
    assert!(parse_dispatch_mode("Hosted").is_err());
}

#[test]
fn migrate_config_rejects_unsupported_target_before_any_read() {
    let root = tempfile::TempDir::new().expect("root");
    let missing = root.path().join("no-such-dir");
    let err = migrate_config(&missing, 99, false).expect_err("bad target");
    assert!(
        err.to_string().contains("unsupported_migration_target:99"),
        "wrong error: {err}"
    );
}

#[test]
fn migrate_config_rejects_missing_root() {
    let root = tempfile::TempDir::new().expect("root");
    let missing = root.path().join("no-such-dir");
    assert!(migrate_config(&missing, 2, false).is_err());
}

#[test]
fn migrate_config_previews_schema2_without_writing() {
    let root = tempfile::TempDir::new().expect("root");
    std::fs::create_dir(root.path().join(".velnor")).expect("config directory");
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\npolicy = 'consumer-v1'\ndefault_branch = 'main'\n",
    )
    .expect("config");
    let text = migrate_config(root.path(), 2, false).expect("preview");
    assert!(text.contains("schema = 2"), "preview: {text}");
    let stored = std::fs::read_to_string(root.path().join(".velnor/config.toml")).expect("stored");
    assert!(stored.contains("schema = 1"), "untouched: {stored}");
}
