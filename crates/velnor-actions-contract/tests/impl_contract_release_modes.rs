//! Publishable-workspace scope mode for the typed release section.
use std::collections::BTreeMap;
use velnor_actions_contract::ContractError;
use velnor_actions_contract::config::RustReleaseConfig;

const FILE: &str = ".velnor/config.toml";

fn enabled_publishable() -> RustReleaseConfig {
    RustReleaseConfig {
        enabled: true,
        publishable_workspace: true,
        ..RustReleaseConfig::default()
    }
}

fn problem_of(config: &RustReleaseConfig) -> String {
    let Err(ContractError::Config {
        key_path, problem, ..
    }) = config.validate(FILE)
    else {
        return String::from("unexpectedly_valid");
    };
    format!("{key_path} {problem}")
}

#[test]
fn publishable_opt_in_satisfies_enabled_without_allowlist() {
    let config = enabled_publishable();
    assert!(config.packages.is_empty());
    assert_eq!(config.validate(FILE), Ok(()));
    assert!(!RustReleaseConfig::default().publishable_workspace);
}

#[test]
fn publishable_opt_in_rejects_combined_allowlist() {
    let mut config = enabled_publishable();
    config.packages = vec!["demo-crate".to_owned()];
    assert_eq!(
        problem_of(&config),
        "stacks.rust.release.publishable_workspace packages_with_publishable"
    );
    config.enabled = false;
    assert!(problem_of(&config).ends_with("packages_with_publishable"));
}

#[test]
fn publishable_opt_in_defers_group_membership_to_emission() {
    let mut config = enabled_publishable();
    config.version_groups = BTreeMap::from([(
        "core".to_owned(),
        vec!["aaa-crate".to_owned(), "demo-crate".to_owned()],
    )]);
    assert_eq!(config.validate(FILE), Ok(()));
    let mut split = config.clone();
    split.version_groups = BTreeMap::from([
        ("one".to_owned(), vec!["demo-crate".to_owned()]),
        ("two".to_owned(), vec!["demo-crate".to_owned()]),
    ]);
    assert!(problem_of(&split).ends_with("member_in_two_groups:demo-crate"));
    let mut unsorted = config.clone();
    unsorted.version_groups = BTreeMap::from([(
        "core".to_owned(),
        vec!["demo-crate".to_owned(), "aaa-crate".to_owned()],
    )]);
    assert!(problem_of(&unsorted).ends_with("must_be_sorted"));
}

#[test]
fn publishable_opt_in_decodes_and_roundtrips() {
    let document = r#"{"enabled":true,"publishable_workspace":true}"#;
    let config: RustReleaseConfig = serde_json::from_str(document).expect("decode");
    assert!(config.publishable_workspace);
    assert_eq!(config.validate(FILE), Ok(()));
    let back: RustReleaseConfig =
        serde_json::from_str(&serde_json::to_string(&config).expect("encode")).expect("roundtrip");
    assert_eq!(back, config);
}
