// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::io::env::Environment;

use crate::fake_docker::FakeDocker;
use crate::support::{Harness, MAIN, PULL_2, temp_dir};

fn seeded(name: &str) -> std::path::PathBuf {
    let dir = temp_dir(name);
    for (path, text) in [
        (
            "cache/index.json",
            r#"{"next_id":0,"records":[],"pull_request_bases":{}}"#,
        ),
        ("cache/archives/blob", "bytes"),
        ("cache/restores.jsonl", "{\"run\":1}\n"),
        ("api/pulls.json", "[]"),
        ("logs/job.log", "log"),
        ("metrics/run.json", "{}"),
        ("events/1.json", "{}"),
        ("keys/control", "token"),
    ] {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(file, text).unwrap_or_else(|e| unreachable!("{e}"));
    }
    dir
}

#[tokio::test]
async fn a_rerun_that_keeps_the_cache_finds_its_entries_but_not_the_old_run_state() {
    let dir = seeded("reset-keeps-cache");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    env.reset(true)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    for kept in ["cache/index.json", "cache/archives/blob", "keys/control"] {
        assert!(env.dir.join(kept).exists(), "{kept}");
    }
    for gone in [
        "cache/restores.jsonl",
        "api/pulls.json",
        "logs/job.log",
        "metrics/run.json",
        "events/1.json",
    ] {
        assert!(!env.dir.join(gone).exists(), "{gone}");
    }
    for directory in ["cache", "api", "events", "logs", "metrics"] {
        assert!(env.dir.join(directory).is_dir(), "{directory}");
    }
}

async fn rerun(harness: Harness, dir: &std::path::Path) -> Harness {
    let docker = FakeDocker::default();
    let env = Environment::at(dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    drop(harness);
    env.reset(true)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    Harness::at(dir.join("cache"), 10_000_000_000).await
}

#[tokio::test]
async fn a_rerun_on_another_default_branch_registers_pull_request_1_and_finds_the_old_entries() {
    let dir = temp_dir("reset-rebase");
    let first = Harness::at(dir.join("cache"), 10_000_000_000).await;
    assert_eq!(first.register_base(1, "main").await, 200);
    assert_eq!(
        first.save(MAIN, "tools", b"saved by run one").await["ok"],
        true
    );
    assert_eq!(first.register_base(1, "trunk").await, 412);

    let second = rerun(first, &dir).await;

    assert_eq!(second.register_base(1, "trunk").await, 200);
    let hit = second.restore(MAIN, "tools").await;
    assert_eq!(hit["ok"], true);
    let url = hit["signed_download_url"].as_str().unwrap_or_default();
    assert_eq!(second.get(url).await.body, b"saved by run one"[..]);
}

#[tokio::test]
async fn a_rerun_forgets_a_reservation_the_old_run_never_committed() {
    let dir = temp_dir("reset-reservation");
    let first = Harness::at(dir.join("cache"), 10_000_000_000).await;
    let entry = serde_json::json!({"key": "crashed", "version": "v"});
    let created = first.twirp(MAIN, "CreateCacheEntry", &entry).await.json();
    let url = created["signed_upload_url"].as_str().unwrap_or_default();
    assert_eq!(first.put(url, b"partial").await.status, 201);
    assert_eq!(first.save(MAIN, "crashed", b"again").await["ok"], false);

    let second = rerun(first, &dir).await;

    assert_eq!(second.save(MAIN, "crashed", b"whole").await["ok"], true);
    let archives = std::fs::read_dir(dir.join("cache/archives")).map_or(0, Iterator::count);
    assert_eq!(archives, 1);
}

#[tokio::test]
async fn a_rerun_forgets_what_a_pull_request_saved_and_keeps_the_default_branch_entries() {
    let dir = temp_dir("reset-pull-request");
    let first = Harness::at(dir.join("cache"), 10_000_000_000).await;
    assert_eq!(first.register_base(2, "main").await, 200);
    assert_eq!(
        first.save(PULL_2, "build", b"pull request 2").await["ok"],
        true
    );
    assert_eq!(
        first.save(MAIN, "tools", b"default branch").await["ok"],
        true
    );

    let second = rerun(first, &dir).await;

    assert_eq!(second.register_base(2, "main").await, 200);
    assert_eq!(second.restore(PULL_2, "build").await["ok"], false);
    assert_eq!(second.restore(MAIN, "tools").await["ok"], true);
    let archives = std::fs::read_dir(dir.join("cache/archives")).map_or(0, Iterator::count);
    assert_eq!(archives, 1);
}

#[tokio::test]
async fn a_run_starts_from_an_empty_cache_unless_it_keeps_it() {
    let dir = seeded("reset-empties-cache");
    let docker = FakeDocker::default();
    let env = Environment::at(&dir, &docker)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    env.reset(false)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    for gone in [
        "cache/index.json",
        "cache/archives/blob",
        "cache/restores.jsonl",
    ] {
        assert!(!env.dir.join(gone).exists(), "{gone}");
    }
    assert!(env.dir.join("keys/control").exists());
}

fn run_args(extra: &[&str]) -> velnor_local::cli::RunArgs {
    use clap::Parser;
    let base = ["velnor-local", "run", "warm", "--dir", "d", "--repo", "r"];
    match velnor_local::cli::Cli::try_parse_from(base.iter().chain(extra)) {
        Ok(velnor_local::cli::Cli {
            command: velnor_local::cli::Command::Run(args),
        }) => args,
        other => unreachable!("{other:?}"),
    }
}

fn shipped(name: &str) -> velnor_local::scenario::Scenario {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../local/scenarios")
        .join(format!("{name}.json"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()));
    velnor_local::scenario::Scenario::parse(&bytes).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_rerun_keeps_the_cache_unless_the_flag_or_the_scenario_asks_for_an_empty_one() {
    let warm = shipped("warm");
    assert!(warm.keeps_cache(run_args(&[]).fresh_cache));
    assert!(!warm.keeps_cache(run_args(&["--fresh-cache"]).fresh_cache));
}

#[test]
fn cold_and_the_cache_probes_start_from_an_empty_cache_and_no_other_scenario_does() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../local/scenarios");
    let mut fresh = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| unreachable!("{e}")) {
        let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_owned();
        if !shipped(&name).keeps_cache(false) {
            fresh.push(name);
        }
    }
    fresh.sort();
    assert_eq!(
        fresh,
        [
            "cold",
            "probe-cache-other-pr",
            "probe-cache-own-pr",
            "probe-cache-pr-reads",
            "probe-cache-restore-keys",
            "probe-cache-save-existing"
        ]
    );
}
