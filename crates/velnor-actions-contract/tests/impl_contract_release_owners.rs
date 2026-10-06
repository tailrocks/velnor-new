//! Exact owner policy cases use synthetic IDs, never inferred identities.

use std::collections::BTreeMap;
use velnor_actions_contract::config::RustReleaseConfig;

fn policy(owners: &[&str]) -> RustReleaseConfig {
    RustReleaseConfig {
        enabled: true,
        packages: vec!["demo-crate".to_owned()],
        expected_owners: BTreeMap::from([(
            "demo-crate".to_owned(),
            owners.iter().map(|owner| (*owner).to_owned()).collect(),
        )]),
        ..RustReleaseConfig::default()
    }
}

#[test]
fn enabled_release_requires_explicit_owner_policy() {
    let mut release = policy(&["team:2", "user:1"]);
    assert_eq!(release.validate("config.toml"), Ok(()));
    release.expected_owners.clear();
    let error = release
        .validate("config.toml")
        .expect_err("policy required");
    assert!(error.to_string().contains("missing_owner_policy"));
    release.enabled = false;
    assert_eq!(release.validate("config.toml"), Ok(()));
}

#[test]
fn owners_require_sorted_unique_nonempty_exact_ids() {
    for owners in [vec![], vec!["user:1", "user:1"], vec!["user:1", "team:2"]] {
        assert!(
            policy(&owners).validate("config.toml").is_err(),
            "{owners:?}"
        );
    }
    for owner in [
        "user:0",
        "team:0",
        "user:01",
        "user:-1",
        "user:+1",
        "user:",
        "user:18446744073709551616",
        "user:1:2",
        "user: 1",
        "org:1",
        "owner/repo",
        "user:alice",
        "team:owner/team",
        "${{ secrets.OWNER }}",
        "https://crates.io/users/1",
        "user:1;id",
        "user:1\n",
    ] {
        let error = policy(&[owner])
            .validate("config.toml")
            .expect_err("unsafe ID");
        assert!(
            error.to_string().contains("invalid_owner_identity"),
            "{error}"
        );
    }
}

#[test]
fn drafted_owner_policy_keeps_package_and_identity_validation() {
    let mut release = policy(&["user:1"]);
    release.enabled = false;
    release.expected_owners = BTreeMap::from([("../escape".to_owned(), vec!["user:1".to_owned()])]);
    assert!(release.validate("config.toml").is_err());
    release.expected_owners = BTreeMap::from([("demo-crate".to_owned(), vec!["alice".to_owned()])]);
    assert!(release.validate("config.toml").is_err());
}

#[test]
fn omitted_policy_defaults_empty_and_exact_policy_roundtrips()
-> Result<(), Box<dyn std::error::Error>> {
    let release: RustReleaseConfig = serde_json::from_str("{}")?;
    assert!(release.expected_owners.is_empty());
    let release = policy(&["team:2", "user:1"]);
    let decoded: RustReleaseConfig = serde_json::from_str(&serde_json::to_string(&release)?)?;
    assert_eq!(release, decoded);
    Ok(())
}
