use std::path::PathBuf;

use velnor_actions_contract::{QualificationCacheLayer, canonical_json_str};

use super::{CacheRootId, CacheRootSet, MAX_ROOTS_JSON_BYTES, expected_paths_for_environment};

fn cargo_roots() -> serde_json::Value {
    serde_json::json!({
        "schema": "velnor.qualification-cache-roots/v1",
        "matrix_key": "matrix-abc123",
        "layer": "cargo_sources",
        "roots": [
            { "id": "cargo_git_db", "path": "/tmp/runner/velnor/cargo/git/db" },
            { "id": "cargo_registry_cache", "path": "/tmp/runner/velnor/cargo/registry/cache" },
            { "id": "cargo_registry_index", "path": "/tmp/runner/velnor/cargo/registry/index" }
        ]
    })
}

fn canonical(value: &serde_json::Value) -> String {
    canonical_json_str(value).expect("canonical root JSON")
}

#[test]
fn parses_and_binds_the_exact_layer_root_set() {
    let encoded = canonical(&cargo_roots());
    let roots = CacheRootSet::parse(
        &encoded,
        "matrix-abc123",
        QualificationCacheLayer::CargoSources,
    )
    .expect("typed roots");

    assert_eq!(roots.matrix_key, "matrix-abc123");
    assert_eq!(roots.layer, QualificationCacheLayer::CargoSources);
    assert_eq!(roots.roots.len(), 3);
    roots
        .validate_expected_paths(&[
            (
                CacheRootId::CargoGitDb,
                PathBuf::from("/tmp/runner/velnor/cargo/git/db"),
            ),
            (
                CacheRootId::CargoRegistryCache,
                PathBuf::from("/tmp/runner/velnor/cargo/registry/cache"),
            ),
            (
                CacheRootId::CargoRegistryIndex,
                PathBuf::from("/tmp/runner/velnor/cargo/registry/index"),
            ),
        ])
        .expect("generated environment paths");
}

#[test]
fn rejects_stale_lane_layer_and_altered_environment_paths() {
    let encoded = canonical(&cargo_roots());
    assert_eq!(
        CacheRootSet::parse(
            &encoded,
            "matrix-other",
            QualificationCacheLayer::CargoSources,
        )
        .expect_err("stale matrix key"),
        "qualification_cache_roots_stale"
    );
    assert_eq!(
        CacheRootSet::parse(
            &encoded,
            "matrix-abc123",
            QualificationCacheLayer::MbxObjects,
        )
        .expect_err("stale layer"),
        "qualification_cache_roots_stale"
    );
    let roots = CacheRootSet::parse(
        &encoded,
        "matrix-abc123",
        QualificationCacheLayer::CargoSources,
    )
    .expect("typed roots");
    assert_eq!(
        roots
            .validate_expected_paths(&[
                (
                    CacheRootId::CargoGitDb,
                    PathBuf::from("/tmp/runner/attacker/git/db"),
                ),
                (
                    CacheRootId::CargoRegistryCache,
                    PathBuf::from("/tmp/runner/velnor/cargo/registry/cache"),
                ),
                (
                    CacheRootId::CargoRegistryIndex,
                    PathBuf::from("/tmp/runner/velnor/cargo/registry/index"),
                ),
            ])
            .expect_err("environment mismatch"),
        "qualification_cache_roots_mismatch"
    );
}

#[test]
fn rejects_unknown_duplicate_missing_and_unsorted_root_records() {
    let mut unknown = cargo_roots();
    unknown["roots"][0]["extra"] = serde_json::json!(true);
    assert_rejected(&unknown, "qualification_cache_roots_invalid");

    let mut unknown_id = cargo_roots();
    unknown_id["roots"][0]["id"] = serde_json::json!("arbitrary");
    assert_rejected(&unknown_id, "qualification_cache_roots_invalid");

    let mut duplicate = cargo_roots();
    duplicate["roots"][1]["id"] = serde_json::json!("cargo_git_db");
    assert_rejected(&duplicate, "qualification_cache_roots_order_invalid");

    let mut missing = cargo_roots();
    missing["roots"].as_array_mut().expect("array").pop();
    assert_rejected(&missing, "qualification_cache_roots_mismatch");

    let mut unsorted = cargo_roots();
    unsorted["roots"].as_array_mut().expect("array").swap(0, 2);
    assert_rejected(&unsorted, "qualification_cache_roots_order_invalid");
}

#[test]
fn rejects_noncanonical_unknown_fields_and_untrusted_paths() {
    let encoded = canonical(&cargo_roots());
    assert_eq!(
        CacheRootSet::parse(
            &format!("{encoded} "),
            "matrix-abc123",
            QualificationCacheLayer::CargoSources,
        )
        .expect_err("noncanonical whitespace"),
        "qualification_cache_roots_noncanonical"
    );

    let mut unknown = cargo_roots();
    unknown["unchecked"] = serde_json::json!(true);
    assert_rejected(&unknown, "qualification_cache_roots_invalid");

    for path in [
        "relative/path",
        "/",
        "/tmp/../outside",
        "/tmp//runner",
        "/tmp/./runner",
        "/tmp/runner\n/outside",
    ] {
        let mut invalid = cargo_roots();
        invalid["roots"][0]["path"] = serde_json::json!(path);
        assert_rejected(&invalid, "qualification_cache_root_path_invalid");
    }
}

#[test]
fn rejects_oversized_root_observations() {
    let oversized = " ".repeat(MAX_ROOTS_JSON_BYTES + 1);
    assert_eq!(
        CacheRootSet::parse(
            &oversized,
            "matrix-abc123",
            QualificationCacheLayer::CargoSources,
        )
        .expect_err("bounded input"),
        "qualification_cache_roots_invalid"
    );
}

#[test]
fn derives_each_supported_layer_from_runner_environment() {
    let runner_temp = PathBuf::from("/tmp/runner");
    let base = BTreeMap::from([
        ("HOME".to_owned(), "/home/runner".to_owned()),
        (
            "CARGO_HOME".to_owned(),
            "/tmp/runner/velnor/cargo".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "/tmp/runner/velnor/cargo".to_owned(),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            "/tmp/runner/velnor/rustup".to_owned(),
        ),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "/tmp/runner/velnor/rustup".to_owned(),
        ),
        (
            "MISE_DATA_DIR".to_owned(),
            "/tmp/runner/velnor/mise".to_owned(),
        ),
        (
            "MBX_CACHE_DIR".to_owned(),
            "/tmp/runner/velnor/mbx".to_owned(),
        ),
        (
            "TF_PLUGIN_CACHE_DIR".to_owned(),
            "/tmp/runner/velnor/tofu-cache/root-0123456789ab".to_owned(),
        ),
        (
            "MISE_TASK_CACHE_DIR".to_owned(),
            "/tmp/runner/velnor/tasks/task-1/cache".to_owned(),
        ),
    ]);
    for layer in [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ] {
        expected_paths_for_environment(layer, &runner_temp, |name| base.get(name).cloned())
            .expect("typed environment roots");
    }
}

#[test]
fn environment_root_paths_fail_closed_on_home_mismatch_and_escape() {
    let runner_temp = PathBuf::from("/tmp/runner");
    let mismatch = BTreeMap::from([("CARGO_HOME".to_owned(), "/tmp/runner/user-cargo".to_owned())]);
    assert_eq!(
        expected_paths_for_environment(
            QualificationCacheLayer::CargoSources,
            &runner_temp,
            |name| mismatch.get(name).cloned(),
        )
        .expect_err("unexpected cargo home"),
        "qualification_cache_root_environment_invalid"
    );

    let escaping = BTreeMap::from([(
        "TF_PLUGIN_CACHE_DIR".to_owned(),
        "/tmp/runner/velnor/tofu-cache/../../outside/root-0123456789ab".to_owned(),
    )]);
    assert_eq!(
        expected_paths_for_environment(
            QualificationCacheLayer::TofuProviders,
            &runner_temp,
            |name| escaping.get(name).cloned(),
        )
        .expect_err("tofu path traversal"),
        "qualification_cache_root_environment_invalid"
    );
}

#[test]
fn mise_tools_requires_the_isolated_runner_temp_data_root() {
    let runner_temp = PathBuf::from("/tmp/runner");
    for mise_data_dir in ["/home/runner/.local/share/mise", "/tmp/runner/user-mise"] {
        let environment = BTreeMap::from([
            ("MISE_DATA_DIR".to_owned(), mise_data_dir.to_owned()),
            (
                "CARGO_HOME".to_owned(),
                "/tmp/runner/velnor/cargo".to_owned(),
            ),
            (
                "MISE_CARGO_HOME".to_owned(),
                "/tmp/runner/velnor/cargo".to_owned(),
            ),
            (
                "RUSTUP_HOME".to_owned(),
                "/tmp/runner/velnor/rustup".to_owned(),
            ),
            (
                "MISE_RUSTUP_HOME".to_owned(),
                "/tmp/runner/velnor/rustup".to_owned(),
            ),
        ]);
        assert_eq!(
            expected_paths_for_environment(
                QualificationCacheLayer::MiseTools,
                &runner_temp,
                |name| environment.get(name).cloned(),
            )
            .expect_err("nonisolated Mise data root"),
            "qualification_cache_root_environment_invalid"
        );
    }
}

#[test]
fn task_result_requires_an_explicit_isolated_task_cache_root() {
    let runner_temp = PathBuf::from("/tmp/runner");
    let environment = BTreeMap::from([(
        "MISE_TASK_CACHE_DIR".to_owned(),
        "/tmp/runner/velnor/tasks/task-1/cache".to_owned(),
    )]);
    assert_eq!(
        expected_paths_for_environment(QualificationCacheLayer::TaskResult, &runner_temp, |name| {
            environment.get(name).cloned()
        },)
        .expect("explicit task env root")[0]
            .1,
        PathBuf::from("/tmp/runner/velnor/tasks/task-1/cache/task-artifacts/v2")
    );

    let missing = BTreeMap::from([(
        "MISE_CACHE_DIR".to_owned(),
        "/tmp/runner/velnor/tasks/task-1/default".to_owned(),
    )]);
    assert_eq!(
        expected_paths_for_environment(QualificationCacheLayer::TaskResult, &runner_temp, |name| {
            missing.get(name).cloned()
        },)
        .expect_err("do not infer task roots from Mise cache"),
        "qualification_cache_root_environment_missing"
    );

    let outside = BTreeMap::from([(
        "MISE_TASK_CACHE_DIR".to_owned(),
        "/tmp/runner/untrusted-cache".to_owned(),
    )]);
    assert_eq!(
        expected_paths_for_environment(QualificationCacheLayer::TaskResult, &runner_temp, |name| {
            outside.get(name).cloned()
        },)
        .expect_err("unrooted task cache"),
        "qualification_cache_root_environment_invalid"
    );
}

fn assert_rejected(value: &serde_json::Value, expected: &'static str) {
    assert_eq!(
        CacheRootSet::parse(
            &canonical(value),
            "matrix-abc123",
            QualificationCacheLayer::CargoSources,
        )
        .expect_err("malformed roots"),
        expected
    );
}

use std::collections::BTreeMap;
