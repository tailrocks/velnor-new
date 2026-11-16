// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use serde_json::json;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::{Command, Output};
use velnor_ci::report::{LaneMetrics, SNAPSHOT};

type ResultTest = Result<(), Box<dyn std::error::Error>>;
fn cli(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("VELNOR_TESTS_RUN", "7")
        .env_remove("GITHUB_TOKEN")
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}
fn read(repo: &Repo) -> LaneMetrics {
    serde_json::from_slice(
        &fs::read(repo.root().join("ci-metrics.json"))
            .unwrap_or_else(|error| unreachable!("{error}")),
    )
    .unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn report_preserves_restored_inputs_after_save_and_measures_failed_cargo_output() -> ResultTest {
    let repo = Repo::new("report-provenance");
    repo.write(
        "Cargo.toml",
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    );
    repo.write("src/lib.rs", "pub fn value() {}\n");
    repo.write(
        "Cargo.lock",
        "version = 4\n[[package]]\nname='demo'\nversion='0.1.0'\n",
    );
    let samples =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../generator/samples");
    for (from, to) in [
        ("full-run-cron/view/mise.toml", "mise.toml"),
        ("full-run-cron/view/mise.lock", "mise.lock"),
        (
            "helper-lock/view/.velnor/rust-ci.lock",
            ".velnor/rust-ci.lock",
        ),
    ] {
        repo.write(to, &fs::read_to_string(samples.join(from))?);
    }
    velnor_gen::io::write_metadata(repo.root())?;
    repo.git(&["add", "."]);
    assert!(cli(&repo, &["inputs", "save"]).status.success());
    assert!(cli(&repo, &["inputs", "restore"]).status.success());
    let before = fs::read(repo.root().join(SNAPSHOT))?;
    repo.write("src/lib.rs", "pub fn changed() {}\n");
    assert!(cli(&repo, &["inputs", "save"]).status.success());
    assert_eq!(fs::read(repo.root().join(SNAPSHOT))?, before);
    let fixture = repo.root().join("cargo-fixture");
    symlink("/bin/sh", &fixture)?;
    let script = format!(
        "printf '\\377\\n'; printf '   Compiling demo v0.1.0 ({})\\n        FAIL [   0.125s] demo::it failing_test\\n     Summary [   0.125s] 2 tests run: 1 passed, 1 failed\\n'; exit 7",
        fs::canonicalize(repo.root())?.display()
    );
    assert_eq!(
        cli(
            &repo,
            &["run", "--", fixture.to_str().ok_or("path")?, "-c", &script]
        )
        .status
        .code(),
        Some(7)
    );
    let cache = json!([{"family":"target","root":".","restore_step":"target_root","save_step":"save_target_root","requested_key":"target-lock-new","lock_prefix":"target-lock-","matched_key":null,"hit":"miss","bytes":null,"usable":false,"usable_same_lock":false,"save_failed":false}]);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("VELNOR_TESTS_RUN", "7")
        .env("VELNOR_REPORT_CACHES", cache.to_string())
        .env(
            "VELNOR_REPORT_STEPS",
            r#"{"target_root":{"outputs":{"cache-matched-key":"target-lock-old"}}}"#,
        )
        .env_remove("GITHUB_TOKEN")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    let metrics = read(&repo);
    assert!(
        !metrics.invocations[0].compilations.is_empty(),
        "{metrics:?}"
    );
    assert_eq!(metrics.invocations[0].compilations[0].package, "demo");
    assert!(metrics.invocations[0].compilations[0].workspace);
    assert!(!metrics.invocations[0].compilations[0].unchanged);
    assert!(metrics.violations.is_empty(), "{:?}", metrics.violations);
    assert_eq!(metrics.tests_run, Some(7));
    assert_eq!(
        metrics.slowest_tests,
        vec![("demo::it failing_test".to_owned(), 0.125)]
    );
    assert!(metrics.invocations[0].seconds > 0.0);
    Ok(())
}

#[test]
fn checker_rejects_missing_malformed_and_duplicate_artifacts_and_accepts_skipped_lanes()
-> ResultTest {
    let repo = Repo::new("report-check");
    let check = |needs: &str| {
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["report", "--check"])
            .arg(repo.root().join("artifacts"))
            .env("VELNOR_REPORT_NEEDS", needs)
            .output()
            .unwrap_or_else(|error| unreachable!("{error}"))
    };
    assert!(check(r#"{"rust":{"result":"skipped"}}"#).status.success());
    assert!(!check(r#"{"rust":{"result":"failure"}}"#).status.success());
    fs::create_dir_all(repo.root().join("artifacts/rust"))?;
    let mut value = crate::report::lane();
    value["caches"] = json!([]);
    fs::write(
        repo.root().join("artifacts/rust/ci-metrics.json"),
        value.to_string(),
    )?;
    assert!(check(r#"{"rust":{"result":"success"}}"#).status.success());
    fs::write(
        repo.root().join("artifacts/rust/ci-metrics.json"),
        "malformed",
    )?;
    assert!(!check(r#"{"rust":{"result":"success"}}"#).status.success());
    Ok(())
}

#[test]
fn failed_setup_still_writes_metrics_with_unknown_provenance() {
    let repo = Repo::new("report-failed-setup");
    assert!(cli(&repo, &["report"]).status.success());
    let metrics = read(&repo);
    assert_ne!(metrics.diagnostics, [] as [String; 0]);
    assert_eq!(metrics.violations, [] as [String; 0]);
}
