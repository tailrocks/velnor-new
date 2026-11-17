// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, fixture};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;
use velnor_ci::report::{LaneMetrics, RESTORE_WITNESS, SNAPSHOT};
use velnor_ci::telemetry::INVOCATIONS;

fn cache(warm: bool) -> Value {
    json!([{"family":"target","root":".","restore_step":"restore","save_step":"save","requested_key":"key","matched_key":"key","hit":"exact","bytes":null,"usable":warm,"usable_same_lock":warm,"save":"not_attempted"}])
}

fn progress(
    repo: &Repo,
    events: &[Value],
    warm: bool,
) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let root = fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["argv"] = json!(["cargo", "check"]);
    invocation["root"] = json!(root);
    invocation["progress"] = json!(events);
    invocation["artifacts"] = json!([]);
    fs::write(repo.root().join(INVOCATIONS), format!("{invocation}\n"))?;
    super::report(
        repo,
        &cache(warm),
        r#"{"restore":{"outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#,
    )
}

fn event(repo: &Repo, kind: &str, package: &str, version: &str, workspace: bool) -> Value {
    let location = workspace.then(|| {
        let package_dir = if repo.root().join(package).join("Cargo.toml").is_file() {
            repo.root().join(package)
        } else {
            repo.root().to_path_buf()
        };
        fs::canonicalize(package_dir).unwrap_or_else(|error| unreachable!("{error}"))
    });
    json!({"kind":kind,"package":package,"version":version,"location":location})
}

fn restore(repo: &Repo) {
    repo.git(&["add", "."]);
    assert!(super::cli(repo, &["inputs", "save"]).status.success());
    assert!(super::cli(repo, &["inputs", "restore"]).status.success());
}

fn check(repo: &Repo) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["report", "--check"])
        .arg(repo.root())
        .env("VELNOR_REPORT_NEEDS", "{}")
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn snapshot_missing_is_cold_but_parse_and_read_errors_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    for case in ["missing", "malformed", "read-error"] {
        let repo = fixture(&format!("report-snapshot-{case}"))?;
        let snapshot = repo.root().join(SNAPSHOT);
        match case {
            "malformed" => {
                fs::create_dir_all(snapshot.parent().ok_or("snapshot parent")?)?;
                fs::write(&snapshot, "invalid\n")?;
            }
            "read-error" => fs::create_dir_all(&snapshot)?,
            _ => {}
        }
        let metrics = progress(&repo, &[], true)?;
        assert_eq!(
            !metrics.verification_errors.is_empty(),
            case != "missing",
            "{case}: {:?}",
            metrics.verification_errors
        );
    }
    Ok(())
}

#[test]
fn malformed_invocation_records_write_metrics_and_fail_check()
-> Result<(), Box<dyn std::error::Error>> {
    for bytes in [b"\xff\n".as_slice(), b"{broken\n"] {
        let repo = fixture("report-invalid-invocation")?;
        fs::write(repo.root().join(INVOCATIONS), bytes)?;
        let output = super::cli(&repo, &["report"]);
        assert!(output.status.success(), "{output:?}");
        assert_ne!(read(&repo).verification_errors, [] as [String; 0]);
        assert!(!check(&repo).status.success());
    }
    let repo = fixture("report-missing-invocation")?;
    let output = super::cli(&repo, &["report"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(read(&repo).verification_errors, [] as [String; 0]);
    assert!(check(&repo).status.success());
    Ok(())
}

#[test]
fn warm_cycle_exempts_a_workspace_rebuild_only_when_a_real_prerequisite_compiled()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = Repo::new("report-real-cycle");
    repo.write_all(&[
        ("Cargo.toml", "[workspace]\nmembers=['a','b']\nresolver='2'\n"),
        ("Cargo.lock", "version = 4\n"),
        ("a/Cargo.toml", "[package]\nname='a'\nversion='0.1.0'\nedition='2024'\n[dev-dependencies]\nb={path='../b'}\n"),
        ("a/src/lib.rs", "pub fn a() {}\n"),
        ("b/Cargo.toml", "[package]\nname='b'\nversion='0.1.0'\nedition='2024'\n[dependencies]\na={path='../a'}\n"),
        ("b/src/lib.rs", "pub fn b() {}\n"),
    ]);
    super::metadata_inputs(&repo)?;
    velnor_gen::io::write_metadata(repo.root())?;
    restore(&repo);
    let a = event(&repo, "Compiling", "a", "0.1.0", true);
    let b = event(&repo, "Compiling", "b", "0.1.0", true);
    let only_origin = progress(&repo, std::slice::from_ref(&a), true)?;
    assert!(
        only_origin
            .violations
            .iter()
            .any(|line| line.contains("a v0.1.0")),
        "{only_origin:?}"
    );
    let with_prerequisite = progress(&repo, &[a, b], true)?;
    assert_eq!(
        with_prerequisite.violations,
        [] as [String; 0],
        "{:?}",
        with_prerequisite.violations
    );
    Ok(())
}

#[test]
fn restore_witness_separates_prior_changes_from_changes_during_warm_cargo()
-> Result<(), Box<dyn std::error::Error>> {
    let before = fixture("report-witness-before")?;
    before.git(&["add", "."]);
    assert!(super::cli(&before, &["inputs", "save"]).status.success());
    before.write("src/lib.rs", "pub fn changed_before_restore() {}\n");
    assert!(super::cli(&before, &["inputs", "restore"]).status.success());
    let before_event = event(&before, "Compiling", "demo", "0.1.0", true);
    let prior_change = progress(&before, std::slice::from_ref(&before_event), true)?;
    assert_eq!(prior_change.verification_errors, [] as [String; 0]);
    assert_eq!(
        prior_change.violations,
        [] as [String; 0],
        "{prior_change:?}"
    );

    let unchanged = fixture("report-witness-unchanged")?;
    restore(&unchanged);
    let unchanged_event = event(&unchanged, "Compiling", "demo", "0.1.0", true);
    let unchanged_metrics = progress(&unchanged, &[unchanged_event], true)?;
    assert!(
        unchanged_metrics
            .violations
            .iter()
            .any(|line| line.contains("demo v0.1.0")),
        "{unchanged_metrics:?}"
    );

    let during = fixture("report-witness-during")?;
    restore(&during);
    assert!(during.root().join(RESTORE_WITNESS).is_file());
    during.write("src/lib.rs", "pub fn changed() {}\n");
    let during_event = event(&during, "Compiling", "demo", "0.1.0", true);
    let changed = progress(&during, &[during_event], true)?;
    assert_ne!(changed.verification_errors, [] as [String; 0]);

    let cold = fixture("report-witness-cold")?;
    cold.write("src/lib.rs", "pub fn changed() {}\n");
    let cold_event = event(&cold, "Compiling", "demo", "0.1.0", true);
    let cold_metrics = progress(&cold, &[cold_event], false)?;
    assert_eq!(
        cold_metrics.verification_errors,
        [] as [String; 0],
        "{cold_metrics:?}"
    );
    Ok(())
}

#[test]
fn warm_dependency_compile_with_symlinked_lock_is_unverifiable()
-> Result<(), Box<dyn std::error::Error>> {
    let cold = fixture("report-symlink-lock-cold")?;
    cold.git(&["rm", "--cached", "Cargo.lock"]);
    fs::remove_file(cold.root().join("Cargo.lock"))?;
    cold.write("lock-source", "version = 4\n");
    symlink("lock-source", cold.root().join("Cargo.lock"))?;
    cold.git(&["add", "."]);
    let dependency = event(&cold, "Compiling", "dependency", "1.0.0", false);
    let cold_metrics = progress(&cold, &[dependency], false)?;
    assert_eq!(
        cold_metrics.verification_errors,
        [] as [String; 0],
        "{cold_metrics:?}"
    );

    let repo = fixture("report-symlink-lock")?;
    repo.git(&["rm", "--cached", "Cargo.lock"]);
    fs::remove_file(repo.root().join("Cargo.lock"))?;
    repo.write("lock-source", "version = 4\n");
    symlink("lock-source", repo.root().join("Cargo.lock"))?;
    restore(&repo);
    let dependency = event(&repo, "Compiling", "dependency", "1.0.0", false);
    let metrics = progress(&repo, &[dependency], true)?;
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("symlink")),
        "{metrics:?}"
    );
    Ok(())
}

fn read(repo: &Repo) -> LaneMetrics {
    serde_json::from_slice(&fs::read(repo.root().join("ci-metrics.json")).unwrap_or_default())
        .unwrap_or_else(|error| unreachable!("{error}"))
}
