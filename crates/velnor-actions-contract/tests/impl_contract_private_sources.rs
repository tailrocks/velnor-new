//! `[private_sources]` and `.velnor/deps.lock` schema cases (HD-7).

use velnor_actions_contract::config::{
    PrivateAssetSource, PrivateGitTreeSource, PrivateSourcesConfig,
};
use velnor_actions_contract::deps_lock::{
    DEPS_LOCK_SCHEMA, DepsLock, LockedAsset, LockedGitTree, cargo_git_source_repository,
    token_repository_allowlist,
};
use velnor_actions_contract::{ContractError, VelnorConfig};

use crate::impl_remed_contract::valid_config;

const FILE: &str = ".velnor/config.toml";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn sources() -> PrivateSourcesConfig {
    PrivateSourcesConfig {
        owner: "ChainArgos".to_owned(),
        app_id_variable: "VELNOR_DEPS_APP_ID".to_owned(),
        app_private_key_secret: "VELNOR_DEPS_APP_PRIVATE_KEY".to_owned(),
        npm_scopes: vec!["@chainargos".to_owned()],
        maven_bundles: vec![PrivateAssetSource {
            id: "processor-shared-libs".to_owned(),
            repository: "ChainArgos/processor-shared".to_owned(),
            tag: "processor-shared-libs-v2.3.0".to_owned(),
            asset: "processor-shared-libs-2.3.0-maven.tar.gz".to_owned(),
            extract_to: None,
        }],
        release_assets: vec![PrivateAssetSource {
            id: "explorer-schema".to_owned(),
            repository: "ChainArgos/blockchain-explorer".to_owned(),
            tag: "explorer-schema-v1.4.0".to_owned(),
            asset: "explorer-schema-1.4.0.tar.gz".to_owned(),
            extract_to: Some("explorer-schema".to_owned()),
        }],
        git_trees: vec![PrivateGitTreeSource {
            id: "platform-history-2025-11".to_owned(),
            repository: "ChainArgos/java-monorepo".to_owned(),
            tag: "platform-history/2025-11".to_owned(),
        }],
    }
}

fn lock() -> DepsLock {
    DepsLock {
        schema: DEPS_LOCK_SCHEMA,
        maven_bundles: vec![LockedAsset {
            id: "processor-shared-libs".to_owned(),
            repository: "ChainArgos/processor-shared".to_owned(),
            tag: "processor-shared-libs-v2.3.0".to_owned(),
            commit: COMMIT.to_owned(),
            asset: "processor-shared-libs-2.3.0-maven.tar.gz".to_owned(),
            sha256: SHA.to_owned(),
            extract_to: None,
        }],
        release_assets: vec![LockedAsset {
            id: "explorer-schema".to_owned(),
            repository: "ChainArgos/blockchain-explorer".to_owned(),
            tag: "explorer-schema-v1.4.0".to_owned(),
            commit: COMMIT.to_owned(),
            asset: "explorer-schema-1.4.0.tar.gz".to_owned(),
            sha256: SHA.to_owned(),
            extract_to: Some("explorer-schema".to_owned()),
        }],
        git_trees: vec![LockedGitTree {
            id: "platform-history-2025-11".to_owned(),
            repository: "ChainArgos/java-monorepo".to_owned(),
            tag: "platform-history/2025-11".to_owned(),
            commit: COMMIT.to_owned(),
        }],
    }
}

type Case = (
    &'static str,
    Box<dyn FnOnce(&mut PrivateSourcesConfig)>,
    &'static str,
);

fn problem(result: Result<(), ContractError>) -> (String, String) {
    match result {
        Err(ContractError::Config {
            key_path, problem, ..
        }) => (key_path, problem),
        other => ("<no config error>".to_owned(), format!("{other:?}")),
    }
}

fn with_sources(edit: impl FnOnce(&mut PrivateSourcesConfig)) -> VelnorConfig {
    let mut private = sources();
    edit(&mut private);
    let mut config = valid_config();
    config.private_sources = Some(private);
    config
}

#[test]
fn valid_declaration_and_lock_pass_and_agree() {
    let config = with_sources(|_| {});
    assert!(config.validate(FILE).is_ok());
    assert!(lock().validate("deps.lock").is_ok());
    assert!(lock().check_matches(&sources(), "deps.lock").is_ok());
}

#[test]
fn config_rejects_each_malformed_field_with_its_key_path() {
    let cases: Vec<Case> = vec![
        (
            "owner",
            Box::new(|c| c.owner = "bad owner".to_owned()),
            "invalid_owner",
        ),
        (
            "app_id_variable",
            Box::new(|c| c.app_id_variable = "GITHUB_APP".to_owned()),
            "invalid_variable_name",
        ),
        (
            "app_private_key_secret",
            Box::new(|c| c.app_private_key_secret = "lower".to_owned()),
            "invalid_secret_name",
        ),
        (
            "npm_scopes",
            Box::new(|c| c.npm_scopes = vec!["chainargos".to_owned()]),
            "invalid_or_duplicate_scope",
        ),
        (
            "maven_bundles[0].repository",
            Box::new(|c| c.maven_bundles[0].repository = "Other/processor-shared".to_owned()),
            "repository_outside_owner",
        ),
        (
            "release_assets[0].extract_to",
            Box::new(|c| c.release_assets[0].extract_to = Some("../escape".to_owned())),
            "invalid_extract_to",
        ),
        (
            "release_assets[0].extract_to",
            Box::new(|c| c.release_assets[0].extract_to = None),
            "missing_extract_to",
        ),
        (
            "maven_bundles[0].extract_to",
            Box::new(|c| c.maven_bundles[0].extract_to = Some("x".to_owned())),
            "maven_bundle_has_fixed_location",
        ),
        (
            "maven_bundles[0].asset",
            Box::new(|c| c.maven_bundles[0].asset = "a/b.tar.gz".to_owned()),
            "invalid_asset_name",
        ),
        (
            "git_trees[0].tag",
            Box::new(|c| c.git_trees[0].tag = "a..b".to_owned()),
            "invalid_tag",
        ),
        (
            "git_trees[0].id",
            Box::new(|c| c.git_trees[0].id = "explorer-schema".to_owned()),
            "invalid_or_duplicate_id",
        ),
    ];
    for (suffix, edit, expected) in cases {
        let (key, found) = problem(with_sources(edit).validate(FILE));
        assert_eq!(key, format!("private_sources.{suffix}"));
        assert!(found.starts_with(expected), "{suffix}: {found}");
    }
}

#[test]
fn lock_rejects_malformed_pins() {
    let mut bad_commit = lock();
    bad_commit.maven_bundles[0].commit = "ABC".to_owned();
    assert_eq!(problem(bad_commit.validate("l")).1, "invalid_commit");
    let mut bad_sha = lock();
    bad_sha.release_assets[0].sha256 = SHA.to_uppercase();
    assert_eq!(problem(bad_sha.validate("l")).1, "invalid_sha256");
    let mut schema = lock();
    schema.schema = 2;
    assert!(matches!(
        schema.validate("l"),
        Err(ContractError::UnsupportedSchema { .. })
    ));
}

#[test]
fn lock_drift_and_strays_are_reported_by_id() {
    let mut moved = lock();
    moved.maven_bundles[0].tag = "processor-shared-libs-v9.9.9".to_owned();
    assert_eq!(
        problem(moved.check_matches(&sources(), "l")).1,
        "deps_lock_drift:processor-shared-libs"
    );
    let mut missing = lock();
    missing.git_trees.clear();
    assert_eq!(
        problem(missing.check_matches(&sources(), "l")).1,
        "deps_lock_drift:platform-history-2025-11"
    );
    let mut stray_config = sources();
    stray_config.release_assets.clear();
    assert_eq!(
        problem(lock().check_matches(&stray_config, "l")).1,
        "deps_lock_extra:explorer-schema"
    );
}

#[test]
fn only_exact_github_sources_under_the_owner_qualify() {
    let owner = "ChainArgos";
    let ok = |s| cargo_git_source_repository(s, owner);
    assert_eq!(
        ok("git+https://github.com/ChainArgos/processor-shared?tag=v1#abc"),
        Some("ChainArgos/processor-shared".to_owned())
    );
    assert_eq!(
        ok("git+https://github.com/ChainArgos/processor-shared.git#abc"),
        Some("ChainArgos/processor-shared".to_owned())
    );
    for hostile in [
        "registry+https://github.com/rust-lang/crates.io-index",
        "git+https://evil.example/ChainArgos/processor-shared#abc",
        "git+https://github.com.evil.example/ChainArgos/processor-shared#abc",
        "git+https://user@github.com/ChainArgos/processor-shared#abc",
        "git+https://github.com:8443/ChainArgos/processor-shared#abc",
        "git+https://github.com/Other/processor-shared#abc",
        "git+https://github.com/ChainArgos/a/b#abc",
        "git+ssh://git@github.com/ChainArgos/processor-shared#abc",
    ] {
        assert_eq!(ok(hostile), None, "{hostile}");
    }
}

#[test]
fn allowlist_is_sorted_unique_and_owner_scoped() {
    let cargo = [
        "git+https://github.com/ChainArgos/processor-shared?tag=a#1",
        "git+https://github.com/ChainArgos/processor-shared?tag=b#2",
        "git+https://github.com/Other/skipped#3",
        "registry+https://github.com/rust-lang/crates.io-index",
    ];
    assert_eq!(
        token_repository_allowlist("ChainArgos", cargo, Some(&lock())),
        ["blockchain-explorer", "java-monorepo", "processor-shared"]
    );
    assert_eq!(token_repository_allowlist("ChainArgos", [], None).len(), 0);
}

#[test]
fn private_sources_round_trip_through_json() {
    let value = serde_json::to_string(&sources()).unwrap_or_default();
    let back: Result<PrivateSourcesConfig, _> = serde_json::from_str(&value);
    assert_eq!(back.ok(), Some(sources()));
    let unknown = r#"{"owner":"a","app_id_variable":"A","app_private_key_secret":"B","token":"x"}"#;
    assert!(serde_json::from_str::<PrivateSourcesConfig>(unknown).is_err());
}
