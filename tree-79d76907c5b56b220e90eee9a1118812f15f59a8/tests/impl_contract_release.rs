//! Typed release config cases (synthetic demo data only).
use std::collections::BTreeMap;
use velnor_actions_contract::config::{BootstrapRelease, ReleaseAuthentication, RustReleaseConfig};
use velnor_actions_contract::{ContractError, canonical_json_str};

const FILE: &str = ".velnor/config.toml";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn valid_release() -> RustReleaseConfig {
    RustReleaseConfig {
        enabled: true,
        manifest_path: "Cargo.toml".to_owned(),
        packages: vec!["demo-crate".to_owned()],
        publishable_workspace: false,
        environment: "demo-publish".to_owned(),
        authentication: ReleaseAuthentication::TrustedPublishing,
        release_pr: true,
        tag_name: "{{ package }}-v{{ version }}".to_owned(),
        bootstrap: None,
        version_groups: BTreeMap::new(),
    }
}

fn valid_bootstrap() -> BootstrapRelease {
    BootstrapRelease {
        package: "demo-crate".to_owned(),
        version: "1.2.3".to_owned(),
        source_sha: SHA.to_owned(),
    }
}

fn bootstrap_release(mutate: impl FnOnce(&mut BootstrapRelease)) -> RustReleaseConfig {
    let mut config = valid_release();
    config.authentication = ReleaseAuthentication::BootstrapToken;
    let mut bootstrap = valid_bootstrap();
    mutate(&mut bootstrap);
    config.bootstrap = Some(bootstrap);
    config
}

fn config_problem(config: &RustReleaseConfig) -> Option<String> {
    match config.validate(FILE) {
        Err(ContractError::Config {
            key_path, problem, ..
        }) => Some(format!("{key_path} {problem}")),
        _ => None,
    }
}

fn decode_problem(document: &str) -> Option<String> {
    let Err(decode) = serde_json::from_str::<RustReleaseConfig>(document) else {
        return None;
    };
    match ContractError::map_decode_error(FILE, &decode.to_string()) {
        ContractError::Config { problem, .. } => Some(problem),
        _ => None,
    }
}

#[test]
fn release_disabled_by_default_and_enabled_needs_allowlist() {
    let default = RustReleaseConfig::default();
    assert!(!default.enabled);
    assert!(default.packages.is_empty());
    assert_eq!(default.validate(FILE), Ok(()));
    assert_eq!(
        default.authentication,
        ReleaseAuthentication::TrustedPublishing
    );
    assert_eq!(valid_release().validate(FILE), Ok(()));
    let mut empty = valid_release();
    empty.packages.clear();
    let got = config_problem(&empty).expect("must reject");
    assert_eq!(got, "stacks.rust.release.packages empty_packages");
}

#[test]
fn release_rejects_unknown_fields_without_shell_yaml_uses() {
    for document in [
        r#"{"enabled":true,"shell":"cargo publish"}"#,
        r#"{"enabled":true,"uses":"some/action@ref"}"#,
        r#"{"enabled":true,"run":["cargo","publish"]}"#,
        r#"{"enabled":true,"bootstrap":{"package":"demo-crate","token":"abc"}}"#,
    ] {
        assert_eq!(
            decode_problem(document).expect("must reject"),
            "unknown_config_field",
            "for {document}"
        );
    }
}

#[test]
fn release_rejects_duplicate_unsorted_and_unsafe_selection() {
    let mut duplicate = valid_release();
    duplicate.packages = vec!["demo-crate".to_owned(), "demo-crate".to_owned()];
    assert!(
        config_problem(&duplicate)
            .expect("must reject")
            .ends_with("duplicate_package")
    );
    let mut unsorted = valid_release();
    unsorted.packages = vec!["demo-crate".to_owned(), "aaa-crate".to_owned()];
    assert!(
        config_problem(&unsorted)
            .expect("must reject")
            .ends_with("must_be_sorted")
    );
    for name in "|../escape|a/b|has space|9bad|-bad|bad!|bad;run|bad$(x)|..".split('|') {
        let mut unsafe_name = valid_release();
        unsafe_name.packages = vec![name.to_owned()];
        let got = config_problem(&unsafe_name).expect("must reject");
        assert_eq!(
            got,
            format!("stacks.rust.release.packages unsafe_package:{name}")
        );
    }
}

#[test]
fn release_rejects_contradictory_authentication_modes() {
    let mut missing = valid_release();
    missing.authentication = ReleaseAuthentication::BootstrapToken;
    let got = config_problem(&missing).expect("must reject");
    assert_eq!(
        got,
        "stacks.rust.release.bootstrap missing_bootstrap_record"
    );
    let mut contradictory = valid_release();
    contradictory.bootstrap = Some(valid_bootstrap());
    let got = config_problem(&contradictory).expect("must reject");
    assert_eq!(
        got,
        "stacks.rust.release.bootstrap contradictory_authentication"
    );
    assert_eq!(bootstrap_release(|_| {}).validate(FILE), Ok(()));
}

#[test]
fn release_rejects_bootstrap_mismatch_fail_closed() {
    let bad_package = bootstrap_release(|bootstrap| bootstrap.package = "../evil".to_owned());
    assert!(
        config_problem(&bad_package)
            .expect("must reject")
            .contains("unsafe_package:")
    );
    for version in ["1.0", "v1.2.3", "1.2.3-beta", "1.2.3+build", "a.b.c", ""] {
        let bad = bootstrap_release(|bootstrap| bootstrap.version = version.to_owned());
        let got = config_problem(&bad).expect("must reject");
        let want = format!("stacks.rust.release.bootstrap.version bad_version:{version}");
        assert_eq!(got, want, "for {version:?}");
    }
    let non_hex = "Z".repeat(40);
    let upper = SHA.to_uppercase();
    for sha in ["abc", non_hex.as_str(), upper.as_str(), ""] {
        let bad = bootstrap_release(|bootstrap| bootstrap.source_sha = sha.to_owned());
        let got = config_problem(&bad).expect("must reject");
        assert_eq!(
            got,
            "stacks.rust.release.bootstrap.source_sha bad_source_sha"
        );
    }
}

#[test]
fn release_validates_manifest_environment_and_tag() {
    for path in "|/abs/Cargo.toml|../up/Cargo.toml|crates/a|a\\Cargo.toml".split('|') {
        let mut bad = valid_release();
        bad.manifest_path = path.to_owned();
        assert!(
            config_problem(&bad)
                .expect("must reject")
                .starts_with("stacks.rust.release.manifest_path ")
        );
    }
    let mut nested = valid_release();
    nested.manifest_path = "crates/demo/Cargo.toml".to_owned();
    assert_eq!(nested.validate(FILE), Ok(()));
    for env in ["", " padded", "bad env!", "../x", "a//b"] {
        let mut bad = valid_release();
        bad.environment = env.to_owned();
        assert!(
            config_problem(&bad)
                .expect("must reject")
                .starts_with("stacks.rust.release.environment ")
        );
    }
    for tag in "v{{ version }}|{{ package }}-v1.0||{{ package }}-$(x)-{{ version }}|{{ package }}-`x`-{{ version }}|{{ package }}-v{{ version }".split('|') {
        let mut bad = valid_release();
        bad.tag_name = tag.to_owned();
        assert!(config_problem(&bad).expect("must reject").starts_with("stacks.rust.release.tag_name "));
    }
}

#[test]
fn release_version_groups_are_non_lockstep_and_allowlist_bound() {
    let mut grouped = valid_release();
    grouped.packages = vec!["aaa-crate".to_owned(), "demo-crate".to_owned()];
    grouped.version_groups = BTreeMap::from([(
        "core".to_owned(),
        vec!["aaa-crate".to_owned(), "demo-crate".to_owned()],
    )]);
    assert_eq!(grouped.validate(FILE), Ok(()));
    let mut unknown = grouped.clone();
    unknown.version_groups = BTreeMap::from([("core".to_owned(), vec!["ghost".to_owned()])]);
    assert!(
        config_problem(&unknown)
            .expect("must reject")
            .ends_with("unknown_package:ghost")
    );
    let mut split = grouped.clone();
    split.version_groups = BTreeMap::from([
        ("one".to_owned(), vec!["demo-crate".to_owned()]),
        ("two".to_owned(), vec!["demo-crate".to_owned()]),
    ]);
    assert!(
        config_problem(&split)
            .expect("must reject")
            .ends_with("member_in_two_groups:demo-crate")
    );
    let mut empty = grouped.clone();
    empty.version_groups = BTreeMap::from([("core".to_owned(), vec![])]);
    assert!(
        config_problem(&empty)
            .expect("must reject")
            .ends_with("empty_group")
    );
    let mut bad_group = grouped;
    let groups = BTreeMap::from([("Bad group".to_owned(), vec!["demo-crate".to_owned()])]);
    bad_group.version_groups = groups;
    assert!(
        config_problem(&bad_group)
            .expect("must reject")
            .ends_with("unsafe_group:Bad group")
    );
}

#[test]
fn release_wired_into_stack_validation_with_key_paths() {
    use velnor_actions_contract::{RustConfiguration, RustStackConfig};
    let stack = RustStackConfig {
        configurations: vec![RustConfiguration {
            name: "default".to_owned(),
            features: vec!["default".to_owned()],
            target: "host".to_owned(),
        }],
        compile_driver: None,
        test_runner: None,
        run_ignored: None,
        custom_tasks: Vec::new(),
        release: valid_release(),
    };
    assert_eq!(stack.validate(FILE), Ok(()));
    let mut bad = stack.clone();
    bad.release.packages.clear();
    let Err(ContractError::Config { key_path, .. }) = bad.validate(FILE) else {
        panic!("empty enabled allowlist must fail through the stack");
    };
    assert_eq!(key_path, "stacks.rust.release.packages");
    assert_eq!(RustStackConfig::default_config().validate(FILE), Ok(()));
}

#[test]
fn release_config_is_deterministic() {
    let first = canonical_json_str(&valid_release()).expect("canonical");
    let second = canonical_json_str(&valid_release()).expect("canonical");
    assert_eq!(first, second);
    let roundtrip: RustReleaseConfig = serde_json::from_str(&first).expect("roundtrip");
    assert_eq!(roundtrip, valid_release());
    assert_eq!(canonical_json_str(&roundtrip).expect("canonical"), first);
}
