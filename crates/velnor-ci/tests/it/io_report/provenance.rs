// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, fixture, report};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::process::Command;
use velnor_ci::inputs::{snapshot_file, witness_file};
use velnor_ci::report::LaneMetrics;
use velnor_ci::telemetry::INVOCATIONS;

mod artifacts;
mod downloads;
mod links;
mod restored_units;
mod roots;
mod targets;

fn cache() -> Value {
    json!([{"family":"target","root":".","restore_step":"restore","save_step":"save"}])
}

fn progress(repo: &Repo, events: &[Value]) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    observation(repo, events, &[])
}

fn observation(
    repo: &Repo,
    events: &[Value],
    artifacts: &[Value],
) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let root = fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["argv"] = json!(["cargo", "check"]);
    invocation["root"] = json!(root);
    invocation["compilations"] = json!([]);
    invocation["progress"] = json!(events);
    invocation["artifacts"] = json!(artifacts);
    fs::write(repo.root().join(INVOCATIONS), format!("{invocation}\n"))?;
    super::report(
        repo,
        &cache(),
        r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#,
    )
}

fn artifact(
    repo: &Repo,
    target: &str,
    kind: &str,
    filename: &str,
    fresh: bool,
    test: bool,
) -> Result<Value, Box<dyn std::error::Error>> {
    let root = fs::canonicalize(repo.root())?;
    Ok(json!({
        "package_id":format!("path+file://{}#demo@0.1.0", root.display()),
        "manifest_path":root.join("Cargo.toml"),
        "target":{"name":target,"kind":[kind],"crate_types":[if test { "bin" } else { "rlib" }],"src_path":root.join("src/lib.rs")},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":test},
        "features":[],"filenames":[filename],"fresh":fresh
    }))
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
        let snapshot = repo.root().join("target").join(snapshot_file(""));
        match case {
            "malformed" => {
                fs::create_dir_all(snapshot.parent().ok_or("snapshot parent")?)?;
                fs::write(&snapshot, "invalid\n")?;
            }
            "read-error" => fs::create_dir_all(&snapshot)?,
            _ => {}
        }
        let metrics = progress(&repo, &[])?;
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
    let only_origin = progress(&repo, std::slice::from_ref(&a))?;
    assert!(
        only_origin
            .violations
            .iter()
            .any(|line| line.contains("a v0.1.0")),
        "{only_origin:?}"
    );
    let with_prerequisite = progress(&repo, &[a, b])?;
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
    let base = fixture("report-witness-base")?;
    base.git(&["add", "."]);
    assert!(super::cli(&base, &["inputs", "save"]).status.success());

    let before = base.fork("report-witness-before");
    before.write("src/lib.rs", "pub fn changed_before_restore() {}\n");
    assert!(super::cli(&before, &["inputs", "restore"]).status.success());
    let before_event = event(&before, "Compiling", "demo", "0.1.0", true);
    let prior_change = progress(&before, std::slice::from_ref(&before_event))?;
    assert_eq!(prior_change.verification_errors, [] as [String; 0]);
    assert!(prior_change.violations.is_empty(), "{prior_change:?}");

    let during = base.fork("report-witness-during");
    restore(&during);
    assert!(
        during
            .root()
            .join("target")
            .join(witness_file(""))
            .is_file()
    );
    during.write("src/lib.rs", "pub fn changed() {}\n");
    let during_event = event(&during, "Compiling", "demo", "0.1.0", true);
    let changed = progress(&during, &[during_event])?;
    assert_ne!(changed.verification_errors, [] as [String; 0]);
    for steps in [
        r#"{"restore":{"outcome":"skipped","outputs":{}}}"#,
        r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-hit":"false"}}}"#,
    ] {
        let cold = report(&during, &cache(), steps)?;
        assert!(!cold.caches[0].usable, "{cold:?}");
        assert!(!cold.caches[0].usable_same_lock, "{cold:?}");
        assert_eq!(cold.verification_errors, [] as [String; 0], "{cold:?}");
    }
    Ok(())
}

#[test]
fn warm_dependency_compile_with_symlinked_lock_is_unverifiable()
-> Result<(), Box<dyn std::error::Error>> {
    let base = fixture("report-symlink-lock-base")?;
    let cold = base.fork("report-symlink-lock-cold");
    let warm = base.fork("report-symlink-lock-warm");
    for repo in [&cold, &warm] {
        repo.git(&["rm", "--cached", "Cargo.lock"]);
        fs::remove_file(repo.root().join("Cargo.lock"))?;
        repo.write("lock-source", "version = 4\n");
        symlink("lock-source", repo.root().join("Cargo.lock"))?;
        repo.git(&["add", "."]);
    }
    let dependency = event(&cold, "Compiling", "dependency", "1.0.0", false);
    let cold_metrics = progress(&cold, &[dependency])?;
    assert_eq!(cold_metrics.invocations[0].root, ".");
    assert_eq!(cold_metrics.invocations[0].compilations.len(), 1);
    assert_eq!(
        cold_metrics.verification_errors,
        [] as [String; 0],
        "{cold_metrics:?}"
    );

    assert!(super::cli(&warm, &["inputs", "save"]).status.success());
    assert!(super::cli(&warm, &["inputs", "restore"]).status.success());
    let dependency = event(&warm, "Compiling", "dependency", "1.0.0", false);
    let metrics = progress(&warm, &[dependency])?;
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("symlink")),
        "{metrics:?}"
    );
    let workspace = event(&warm, "Compiling", "demo", "0.1.0", true);
    let metrics = progress(&warm, &[workspace])?;
    assert_ne!(metrics.verification_errors, [] as [String; 0]);

    let quiet = artifact(
        &warm,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        false,
        false,
    )?;
    let metrics = observation(&warm, &[], &[quiet])?;
    assert_ne!(metrics.verification_errors, [] as [String; 0]);
    let cold = report(
        &warm,
        &cache(),
        r#"{"restore":{"outcome":"skipped","outputs":{}}}"#,
    )?;
    assert_eq!(cold.invocations[0].compilations.len(), 1);
    assert_eq!(
        cold.invocations[0].artifacts[0].manifest_path,
        "checkout/Cargo.toml"
    );
    assert!(!cold.caches[0].usable, "{cold:?}");
    assert!(!cold.caches[0].usable_same_lock, "{cold:?}");
    assert_eq!(cold.verification_errors, [] as [String; 0], "{cold:?}");

    let fresh = artifact(
        &warm,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        true,
        false,
    )?;
    let workspace = event(&warm, "Compiling", "demo", "0.1.0", true);
    let metrics = observation(&warm, &[workspace], &[fresh])?;
    assert_eq!(metrics.verification_errors, [] as [String; 0]);
    assert!(metrics.invocations[0].compilations.is_empty());
    Ok(())
}

#[test]
fn unhosted_origin_cannot_place_a_hashed_full_run_schedule()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = fixture("report-unhosted-origin")?;
    repo.write(".velnor/rust-ci.toml", "full_run = \"weekly\"\n");
    repo.git(&["add", "."]);
    repo.git(&["remote", "add", "origin", "file:///nonexistent/demo.git"]);
    let metrics = observation(&repo, &[], &[])?;
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|error| error.contains("hosted repository")),
        "{metrics:?}"
    );
    Ok(())
}

fn read(repo: &Repo) -> LaneMetrics {
    serde_json::from_slice(&fs::read(repo.root().join("ci-metrics.json")).unwrap_or_default())
        .unwrap_or_else(|error| unreachable!("{error}"))
}
