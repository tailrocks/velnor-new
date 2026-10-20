use std::collections::BTreeSet;
use std::fs;

use velnor_actions_contract::config::{PrivateAssetSource, PrivateSourcesConfig};
use velnor_actions_contract::deps_lock::{DEPS_LOCK_SCHEMA, DepsLock, LockedAsset};

use super::*;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA: &str = "89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567";

fn config() -> PrivateSourcesConfig {
    PrivateSourcesConfig {
        owner: "ChainArgos".to_owned(),
        app_id_variable: "A".to_owned(),
        app_private_key_secret: "B".to_owned(),
        npm_scopes: Vec::new(),
        maven_bundles: Vec::new(),
        release_assets: vec![PrivateAssetSource {
            id: "schema".to_owned(),
            repository: "ChainArgos/blockchain-explorer".to_owned(),
            tag: "v1".to_owned(),
            asset: "s.tar.gz".to_owned(),
            extract_to: Some("schema".to_owned()),
        }],
        git_trees: Vec::new(),
    }
}

fn lock() -> DepsLock {
    DepsLock {
        schema: DEPS_LOCK_SCHEMA,
        maven_bundles: vec![LockedAsset {
            id: "libs".to_owned(),
            repository: "ChainArgos/processor-shared".to_owned(),
            tag: "t".to_owned(),
            commit: COMMIT.to_owned(),
            asset: "l.tar.gz".to_owned(),
            sha256: SHA.to_owned(),
            extract_to: None,
        }],
        release_assets: vec![LockedAsset {
            id: "schema".to_owned(),
            repository: "ChainArgos/blockchain-explorer".to_owned(),
            tag: "v1".to_owned(),
            commit: COMMIT.to_owned(),
            asset: "s.tar.gz".to_owned(),
            sha256: SHA.to_owned(),
            extract_to: Some("schema".to_owned()),
        }],
        git_trees: Vec::new(),
    }
}

fn kinds(list: &[SourceKind]) -> BTreeSet<SourceKind> {
    list.iter().copied().collect()
}

#[test]
fn kinds_round_trip_their_canonical_spellings() {
    for kind in [
        SourceKind::Cargo,
        SourceKind::GitTrees,
        SourceKind::MavenBundles,
        SourceKind::ReleaseAssets,
    ] {
        assert_eq!(SourceKind::parse(kind.as_str()), Some(kind));
    }
    assert_eq!(SourceKind::parse("Cargo"), None);
    assert_eq!(
        velnor_actions_workflow_renderer::private_source_steps::PRIVATE_SOURCE_KINDS,
        ["cargo", "git-trees", "maven-bundles", "release-assets"],
        "renderer and orchestrator share one kind vocabulary, in sorted order"
    );
}

#[test]
fn cargo_lock_closure_reads_only_exact_owner_git_sources() -> Result<(), OrchestratorError> {
    let dir = tempfile::tempdir().map_err(|e| internal(&e.to_string()))?;
    fs::write(
        dir.path().join("Cargo.lock"),
        r#"
version = 4
[[package]]
name = "a"
version = "0.1.0"
[[package]]
name = "shared"
version = "1.0.0"
source = "git+https://github.com/ChainArgos/processor-shared?tag=v1#0123456789abcdef0123456789abcdef01234567"
[[package]]
name = "dup"
version = "1.0.0"
source = "git+https://github.com/ChainArgos/processor-shared.git?rev=abc#0123456789abcdef0123456789abcdef01234567"
[[package]]
name = "evil"
version = "1.0.0"
source = "git+https://github.com.evil.example/ChainArgos/steal#0123456789abcdef0123456789abcdef01234567"
[[package]]
name = "other"
version = "1.0.0"
source = "git+https://github.com/Other/thing#0123456789abcdef0123456789abcdef01234567"
[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#,
    )
    .map_err(|e| internal(&e.to_string()))?;
    let found = cargo_lock_repositories(dir.path(), "", "ChainArgos")?;
    assert_eq!(found.into_iter().collect::<Vec<_>>(), ["processor-shared"]);
    assert!(cargo_lock_repositories(dir.path(), "missing", "ChainArgos")?.is_empty());
    fs::write(dir.path().join("Cargo.lock"), "not = [toml")
        .map_err(|e| internal(&e.to_string()))?;
    assert!(cargo_lock_repositories(dir.path(), "", "ChainArgos").is_err());
    Ok(())
}

#[test]
fn lock_repositories_follow_the_requested_kinds() {
    let lock = lock();
    let names = |list: &[SourceKind]| -> Vec<String> {
        lock_repositories(Some(&lock), "ChainArgos", &kinds(list))
            .into_iter()
            .collect()
    };
    assert_eq!(names(&[SourceKind::Cargo]), Vec::<String>::new());
    assert_eq!(names(&[SourceKind::ReleaseAssets]), ["blockchain-explorer"]);
    assert_eq!(names(&[SourceKind::MavenBundles]), ["processor-shared"]);
    assert_eq!(
        names(&[SourceKind::ReleaseAssets, SourceKind::MavenBundles]),
        ["blockchain-explorer", "processor-shared"]
    );
    assert!(lock_repositories(None, "ChainArgos", &kinds(&[SourceKind::ReleaseAssets])).is_empty());
    assert!(
        lock_repositories(Some(&lock), "Other", &kinds(&[SourceKind::MavenBundles])).is_empty()
    );
}

#[test]
fn job_allowlist_unions_cargo_and_locked_sources_sorted() -> Result<(), OrchestratorError> {
    let dir = tempfile::tempdir().map_err(|e| internal(&e.to_string()))?;
    fs::write(
        dir.path().join("Cargo.lock"),
        "[[package]]\nname = \"s\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/eth-domain#0123456789abcdef0123456789abcdef01234567\"\n",
    )
    .map_err(|e| internal(&e.to_string()))?;
    let all = job_allowlist(
        dir.path(),
        &config(),
        Some(&lock()),
        &[String::new()],
        &kinds(&[SourceKind::Cargo, SourceKind::ReleaseAssets]),
    )?;
    assert_eq!(all, ["blockchain-explorer", "eth-domain"]);
    let cargo_only = job_allowlist(
        dir.path(),
        &config(),
        Some(&lock()),
        &[String::new()],
        &kinds(&[SourceKind::Cargo]),
    )?;
    assert_eq!(cargo_only, ["eth-domain"]);
    Ok(())
}
