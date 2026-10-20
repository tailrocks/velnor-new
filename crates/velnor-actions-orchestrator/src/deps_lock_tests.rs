use std::collections::BTreeMap;

use velnor_actions_contract::config::{
    PrivateAssetSource, PrivateGitTreeSource, PrivateSourcesConfig,
};

use super::*;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA: &str = "89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567";

struct Fixture(BTreeMap<(String, String), String>);

impl SourceResolver for Fixture {
    fn tag_commit(&self, repository: &str, tag: &str) -> Result<String, OrchestratorError> {
        self.0
            .get(&(repository.to_owned(), tag.to_owned()))
            .cloned()
            .ok_or_else(|| internal("fixture_missing_tag"))
    }

    fn release_asset(
        &self,
        _repository: &str,
        _tag: &str,
        _asset: &str,
    ) -> Result<AssetObservation, OrchestratorError> {
        Ok(AssetObservation {
            sha256: SHA.to_owned(),
        })
    }
}

fn config() -> PrivateSourcesConfig {
    PrivateSourcesConfig {
        owner: "ChainArgos".to_owned(),
        app_id_variable: "VELNOR_DEPS_APP_ID".to_owned(),
        app_private_key_secret: "VELNOR_DEPS_APP_PRIVATE_KEY".to_owned(),
        npm_scopes: Vec::new(),
        maven_bundles: vec![PrivateAssetSource {
            id: "libs".to_owned(),
            repository: "ChainArgos/processor-shared".to_owned(),
            tag: "libs-v1".to_owned(),
            asset: "libs-1-maven.tar.gz".to_owned(),
            extract_to: None,
        }],
        release_assets: Vec::new(),
        git_trees: vec![PrivateGitTreeSource {
            id: "history".to_owned(),
            repository: "ChainArgos/java-monorepo".to_owned(),
            tag: "platform-history/2025-11".to_owned(),
        }],
    }
}

fn fixture() -> Fixture {
    Fixture(BTreeMap::from([
        (
            (
                "ChainArgos/processor-shared".to_owned(),
                "libs-v1".to_owned(),
            ),
            COMMIT.to_owned(),
        ),
        (
            (
                "ChainArgos/java-monorepo".to_owned(),
                "platform-history/2025-11".to_owned(),
            ),
            COMMIT.to_owned(),
        ),
    ]))
}

#[test]
fn lock_is_built_in_declaration_order_and_round_trips_byte_exact() -> Result<(), OrchestratorError>
{
    let lock = build_lock(&config(), &fixture())?;
    assert_eq!(lock.maven_bundles[0].commit, COMMIT);
    assert_eq!(lock.maven_bundles[0].sha256, SHA);
    assert_eq!(lock.git_trees[0].tag, "platform-history/2025-11");
    let text = render_lock(&lock)?;
    assert!(text.starts_with(LOCK_HEADER));
    let parsed: DepsLock = toml::from_str(&text).map_err(|err| internal(&err.to_string()))?;
    assert_eq!(parsed, lock);
    assert_eq!(render_lock(&parsed)?, text, "rendering is deterministic");
    Ok(())
}

#[test]
fn a_missing_tag_fails_the_whole_lock() {
    let empty = Fixture(BTreeMap::new());
    assert!(build_lock(&config(), &empty).is_err());
}

#[test]
fn commit_responses_must_be_forty_lowercase_hex() {
    assert_eq!(
        parse_commit(format!("{{\"sha\":\"{COMMIT}\"}}").as_bytes()).ok(),
        Some(COMMIT.to_owned())
    );
    for bad in [
        r#"{"sha":"ABC"}"#,
        r#"{"sha":"0123456789ABCDEF0123456789ABCDEF01234567"}"#,
        "{}",
        "not json",
    ] {
        assert!(parse_commit(bad.as_bytes()).is_err(), "{bad}");
    }
}

fn release(draft: bool, assets: &str) -> String {
    format!("{{\"draft\":{draft},\"assets\":{assets}}}")
}

#[test]
fn release_assets_must_be_unique_uploaded_and_carry_a_valid_digest() {
    let one = format!(r#"[{{"id":7,"name":"a.tgz","state":"uploaded","digest":"sha256:{SHA}"}}]"#);
    assert_eq!(
        parse_release_asset(release(false, &one).as_bytes(), "a.tgz").ok(),
        Some(FoundAsset {
            id: 7,
            sha256: Some(SHA.to_owned())
        })
    );
    let no_digest = r#"[{"id":8,"name":"a.tgz","state":"uploaded","digest":null}]"#;
    assert_eq!(
        parse_release_asset(release(false, no_digest).as_bytes(), "a.tgz").ok(),
        Some(FoundAsset {
            id: 8,
            sha256: None
        })
    );
    let cases = [
        (true, one.clone(), "a.tgz"),
        (false, one.clone(), "other.tgz"),
        (
            false,
            r#"[{"id":1,"name":"a.tgz","state":"starter","digest":null}]"#.to_owned(),
            "a.tgz",
        ),
        (
            false,
            r#"[{"id":1,"name":"a.tgz","state":"uploaded","digest":"md5:abc"}]"#.to_owned(),
            "a.tgz",
        ),
        (
            false,
            r#"[{"id":1,"name":"a.tgz","state":"uploaded","digest":null},{"id":2,"name":"a.tgz","state":"uploaded","digest":null}]"#.to_owned(),
            "a.tgz",
        ),
    ];
    for (draft, assets, name) in cases {
        assert!(
            parse_release_asset(release(draft, &assets).as_bytes(), name).is_err(),
            "{draft} {assets} {name}"
        );
    }
}

#[test]
fn route_segments_encode_everything_but_path_safe_bytes() {
    assert_eq!(
        encode("platform-history/2025-11"),
        "platform-history/2025-11"
    );
    assert_eq!(encode("a b?c#d"), "a%20b%3Fc%23d");
}

#[test]
fn the_lock_is_written_atomically_and_read_back() -> Result<(), OrchestratorError> {
    let root = tempfile::tempdir().map_err(|err| internal(&err.to_string()))?;
    std::fs::create_dir_all(root.path().join(".velnor"))
        .map_err(|err| internal(&err.to_string()))?;
    assert!(read_deps_lock(root.path())?.is_none());
    let text = render_lock(&build_lock(&config(), &fixture())?)?;
    write_lock(root.path(), &text)?;
    let read = read_deps_lock(root.path())?;
    assert_eq!(read.map(|lock| lock.maven_bundles.len()), Some(1));
    std::fs::write(
        root.path().join(DEPS_LOCK_PATH),
        "schema = 1\nunknown = true\n",
    )
    .map_err(|err| internal(&err.to_string()))?;
    assert!(
        read_deps_lock(root.path()).is_err(),
        "unknown keys rejected"
    );
    Ok(())
}
