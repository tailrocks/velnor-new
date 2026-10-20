use super::*;

#[test]
fn allowlist_drift_stops_before_any_credentialed_call() {
    let world = world();
    for repositories in [
        &["blockchain-explorer", "eth-domain", "java-monorepo"][..],
        &[
            "a-new-repo",
            "blockchain-explorer",
            "eth-domain",
            "java-monorepo",
            "processor-shared",
        ][..],
        &[][..],
    ] {
        let request = request(&world, repositories, &ALL);
        assert_eq!(
            problem(execute(&request, &world.fake)),
            "internal: private_allowlist_drift"
        );
    }
    assert!(world.fake.downloads.borrow().is_empty());
    assert!(world.fake.cargo.borrow().is_empty());
}

#[test]
fn a_pull_request_adding_a_private_cargo_source_fails_before_the_token_is_used() {
    let world = world();
    fs::write(
        world.workspace.path().join("Cargo.lock"),
        "[[package]]\nname = \"d\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/eth-domain#4444444444444444444444444444444444444444\"\n[[package]]\nname = \"x\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/payments-secret#5555555555555555555555555555555555555555\"\n",
    )
    .expect("cargo lock");
    let request = request(&world, &ALL_REPOS, &ALL);
    assert_eq!(
        problem(execute(&request, &world.fake)),
        "internal: private_allowlist_drift"
    );
    assert!(world.fake.cargo.borrow().is_empty() && world.fake.downloads.borrow().is_empty());
}

#[test]
fn a_source_replacement_or_lookalike_host_never_widens_the_allowlist() {
    let world = world();
    fs::write(
        world.workspace.path().join("Cargo.lock"),
        "[[package]]\nname = \"x\"\nversion = \"1\"\nsource = \"git+https://github.com.evil.example/ChainArgos/payments#5555555555555555555555555555555555555555\"\n",
    )
    .expect("cargo lock");
    let request = request(
        &world,
        &["blockchain-explorer", "java-monorepo", "processor-shared"],
        &ALL,
    );
    execute(&request, &world.fake)
        .expect("lookalike hosts receive no credential and add no repository");
}

#[test]
fn a_moved_tag_is_refused_without_downloading_the_asset() {
    let world = world();
    world.fake.serve(
        "repos/ChainArgos/blockchain-explorer/commits/refs/tags/schema-v1",
        format!("{{\"sha\":\"{}\"}}", "9".repeat(40)),
    );
    let request = request(
        &world,
        &["blockchain-explorer"],
        &[SourceKind::ReleaseAssets],
    );
    assert_eq!(
        problem(execute(&request, &world.fake)),
        "internal: private_tag_moved:schema"
    );
    assert!(
        !world
            .fake
            .downloads
            .borrow()
            .iter()
            .any(|r| r.contains("releases/assets"))
    );
}

#[test]
fn a_tampered_asset_fails_the_sha256_check_and_extracts_nothing() {
    let world = world();
    world.fake.serve(
        "repos/ChainArgos/blockchain-explorer/releases/assets/42",
        tar_gz(&[("schema.json", b"{\"v\":666}")]),
    );
    let request = request(
        &world,
        &["blockchain-explorer"],
        &[SourceKind::ReleaseAssets],
    );
    assert_eq!(
        problem(execute(&request, &world.fake)),
        "internal: private_asset_sha256_mismatch:schema"
    );
    assert!(
        !world
            .workspace
            .path()
            .join(".velnor/deps/explorer-schema")
            .exists()
    );
}

#[test]
fn a_republished_digest_that_differs_from_the_lock_is_refused() {
    let world = world();
    world.fake.serve(
        "repos/ChainArgos/blockchain-explorer/releases/tags/schema-v1",
        release_json(42, "schema-1.tar.gz", b"different bytes"),
    );
    let request = request(
        &world,
        &["blockchain-explorer"],
        &[SourceKind::ReleaseAssets],
    );
    assert_eq!(
        problem(execute(&request, &world.fake)),
        "internal: private_asset_digest_changed:schema"
    );
}

#[test]
fn hostile_archives_cannot_write_outside_the_destination() {
    for (label, archive) in [
        ("traversal", hostile_tar_gz("../../escape.txt", None)),
        ("absolute", hostile_tar_gz("/tmp/velnor-escape.txt", None)),
        ("symlink", hostile_tar_gz("link", Some("../../etc"))),
    ] {
        let world = world();
        let sha256 = sha(&archive);
        fs::write(
            world.workspace.path().join(".velnor/deps.lock"),
            crate::config::deps_lock::render_lock(&{
                let mut lock = read_deps_lock(world.workspace.path())
                    .expect("read")
                    .expect("lock");
                lock.release_assets[0].sha256 = sha256.clone();
                lock
            })
            .expect("lock"),
        )
        .expect("write");
        world.fake.serve(
            "repos/ChainArgos/blockchain-explorer/releases/tags/schema-v1",
            release_json(42, "schema-1.tar.gz", &archive),
        );
        world.fake.serve(
            "repos/ChainArgos/blockchain-explorer/releases/assets/42",
            archive,
        );
        let request = request(
            &world,
            &["blockchain-explorer"],
            &[SourceKind::ReleaseAssets],
        );
        let refused = execute(&request, &world.fake).expect_err("must be refused");
        // The pins are consistent, so the refusal must come from the archive
        // reader and not from an earlier check.
        assert!(
            matches!(refused, OrchestratorError::UnsafePath { .. })
                || refused.to_string().contains("archive"),
            "{label} must be stopped by the archive reader: {refused}"
        );
        assert!(
            !world.temp.path().join("escape.txt").exists()
                && !world
                    .temp
                    .path()
                    .parent()
                    .is_some_and(|p| p.join("escape.txt").exists())
                && !Path::new("/tmp/velnor-escape.txt").exists(),
            "{label} wrote outside"
        );
        assert!(
            !world
                .workspace
                .path()
                .join(".velnor/deps/explorer-schema")
                .exists(),
            "{label}"
        );
    }
}

#[test]
fn missing_lock_owner_mismatch_and_missing_table_fail_closed() {
    let world = world();
    fs::remove_file(world.workspace.path().join(".velnor/deps.lock")).expect("rm");
    assert_eq!(
        problem(execute(&request(&world, &ALL_REPOS, &ALL), &world.fake)),
        "internal: deps_lock_missing"
    );
    let world = self::world();
    let mut other = request(&world, &ALL_REPOS, &ALL);
    other.owner = "Other".to_owned();
    assert_eq!(
        problem(execute(&other, &world.fake)),
        "internal: private_fetch_owner_mismatch"
    );
    fs::write(
        world.workspace.path().join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\n",
    )
    .expect("config");
    assert_eq!(
        problem(execute(&request(&world, &ALL_REPOS, &ALL), &world.fake)),
        "internal: private_fetch_without_private_sources"
    );
}

#[test]
fn a_lock_that_drifted_from_the_declarations_is_refused() {
    let world = world();
    let mut lock = read_deps_lock(world.workspace.path())
        .expect("read")
        .expect("lock");
    lock.release_assets[0].tag = "schema-v2".to_owned();
    fs::write(
        world.workspace.path().join(".velnor/deps.lock"),
        crate::config::deps_lock::render_lock(&lock).expect("lock"),
    )
    .expect("write");
    let message = problem(execute(&request(&world, &ALL_REPOS, &ALL), &world.fake));
    assert!(message.contains("deps_lock_drift:schema"), "{message}");
    assert!(world.fake.downloads.borrow().is_empty());
    let _ = &world.schema_archive;
}

#[test]
fn a_planted_deps_symlink_or_stale_content_never_redirects_extraction() {
    let world = world();
    let outside = tempfile::tempdir().expect("outside");
    let deps = world.workspace.path().join(".velnor/deps");
    std::os::unix::fs::symlink(outside.path(), &deps).expect("symlink");
    let request = request(
        &world,
        &["blockchain-explorer"],
        &[SourceKind::ReleaseAssets],
    );
    execute(&request, &world.fake).expect("fetch");
    assert_eq!(
        fs::read_dir(outside.path()).expect("outside").count(),
        0,
        "nothing written through the planted link"
    );
    assert!(
        fs::symlink_metadata(&deps).expect("deps").is_dir(),
        "the link was replaced by a real directory"
    );
    assert!(deps.join("explorer-schema/schema.json").is_file());
}

#[test]
fn a_velnor_directory_that_is_a_symlink_is_refused() {
    let world = world();
    let outside = tempfile::tempdir().expect("outside");
    let velnor = world.workspace.path().join(".velnor");
    fs::rename(&velnor, outside.path().join("real")).expect("move");
    std::os::unix::fs::symlink(outside.path().join("real"), &velnor).expect("symlink");
    let request = request(
        &world,
        &["blockchain-explorer"],
        &[SourceKind::ReleaseAssets],
    );
    // The config loader refuses first; the deps root check is the second wall.
    assert!(execute(&request, &world.fake).is_err());
    assert!(!outside.path().join("real/deps").exists());
}
