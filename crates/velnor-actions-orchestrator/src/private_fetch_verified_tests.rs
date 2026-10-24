use super::*;

fn marker(world: &World) -> std::path::PathBuf {
    world.workspace.path().join(".velnor/deps/verified.json")
}

#[test]
fn a_full_fetch_writes_the_verified_marker_with_every_pin_sorted() {
    let world = world();
    let request = request(&world, &ALL_REPOS, &ALL);
    execute(&request, &world.fake).expect("The private fetch must succeed.");
    let text = fs::read_to_string(marker(&world)).expect("The verified marker must be readable.");
    assert!(text.ends_with("}\n"));
    let schema_sha = sha(&tar_gz(&[
        ("schema.json", b"{\"v\":1}"),
        ("sub/readme.md", b"hi"),
    ]));
    let maven_sha = sha(&tar_gz(&[("com/acme/lib/1/lib-1.jar", b"jar")]));
    let expected = serde_json::json!({
        "schema": 1,
        "sources": [
            {
                "kind": "git-trees",
                "id": "history",
                "path": "git/history",
                "repository": "ChainArgos/java-monorepo",
                "tag": "platform-history/2025-11",
                "commit": MONO_COMMIT,
            },
            {
                "kind": "maven-bundles",
                "id": "libs",
                "path": "maven/libs",
                "repository": "ChainArgos/processor-shared",
                "tag": "libs-v1",
                "commit": SHARED_COMMIT,
                "asset": "libs-1-maven.tar.gz",
                "sha256": maven_sha,
            },
            {
                "kind": "release-assets",
                "id": "schema",
                "path": "explorer-schema",
                "repository": "ChainArgos/blockchain-explorer",
                "tag": "schema-v1",
                "commit": EXPLORER_COMMIT,
                "asset": "schema-1.tar.gz",
                "sha256": schema_sha,
            },
        ],
    });
    let actual: serde_json::Value =
        serde_json::from_str(&text).expect("The marker JSON must parse.");
    assert_eq!(actual, expected);
    let lock = read_deps_lock(world.workspace.path())
        .expect("The dependency lock must be readable.")
        .expect("The dependency lock must exist.");
    for entry in actual["sources"]
        .as_array()
        .expect("The marker must contain a sources array.")
    {
        let id = entry["id"]
            .as_str()
            .expect("The source ID must be a string.");
        let locked_commit = lock
            .maven_bundles
            .iter()
            .chain(&lock.release_assets)
            .find(|a| a.id == id)
            .map(|a| (a.commit.clone(), Some(a.sha256.clone())))
            .or_else(|| {
                lock.git_trees
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| (t.commit.clone(), None))
            })
            .expect("The lock must contain every marker entry.");
        assert_eq!(entry["commit"], locked_commit.0);
        assert_eq!(entry["sha256"].as_str().map(str::to_owned), locked_commit.1);
    }
}

#[test]
fn the_marker_lists_only_what_this_job_verified() {
    let world = world();
    let request = request(
        &world,
        &["blockchain-explorer"],
        &[SourceKind::ReleaseAssets],
    );
    execute(&request, &world.fake).expect("The private fetch must succeed.");
    let actual: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(marker(&world)).expect("The verified marker must be readable."),
    )
    .expect("The marker JSON must parse.");
    let ids: Vec<&str> = actual["sources"]
        .as_array()
        .expect("The marker must contain a sources array.")
        .iter()
        .map(|e| e["id"].as_str().expect("The source ID must be a string."))
        .collect();
    assert_eq!(ids, ["schema"]);
}

#[test]
fn a_cargo_only_fetch_writes_nothing_into_the_workspace() {
    let world = world();
    let request = request(&world, &["eth-domain"], &[SourceKind::Cargo]);
    execute(&request, &world.fake).expect("The private fetch must succeed.");
    assert!(!marker(&world).exists());
    assert!(!world.workspace.path().join(".velnor/deps").exists());
}

#[test]
fn a_planted_marker_is_replaced_and_a_bundle_on_the_marker_path_fails_closed() {
    let world = world();
    let deps = world.workspace.path().join(".velnor/deps");
    fs::create_dir_all(&deps).expect("The dependency directory must exist.");
    fs::write(deps.join("verified.json"), "{\"schema\":1,\"sources\":[]}")
        .expect("The test marker must be written.");
    let request = request(
        &world,
        &["blockchain-explorer"],
        &[SourceKind::ReleaseAssets],
    );
    execute(&request, &world.fake).expect("The private fetch must succeed.");
    assert!(
        fs::read_to_string(marker(&world))
            .expect("The verified marker must be readable.")
            .contains("\"schema-v1\""),
        "The fetch must replace the marker supplied by the checkout."
    );
    fs::create_dir_all(deps.join("verified.json")).ok();
    let verified = Verified::new(vec![]);
    verified
        .write(&deps)
        .expect("An empty fetch must write no files.");
    let collision =
        crate::config::private_fetch_verified::Verified::new(vec![VerifiedSource::tree(
            &velnor_actions_contract::deps_lock::LockedGitTree {
                id: "t".to_owned(),
                repository: "ChainArgos/x".to_owned(),
                tag: "v1".to_owned(),
                commit: MONO_COMMIT.to_owned(),
            },
            "git/t",
        )]);
    assert_eq!(
        collision.write(&deps).map_err(|e| e.to_string()),
        Err("internal: private_verified_marker_collision".to_owned())
    );
}
