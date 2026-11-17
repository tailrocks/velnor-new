// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, fixture};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
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
    observation(repo, events, &[], warm)
}

fn observation(
    repo: &Repo,
    events: &[Value],
    artifacts: &[Value],
    warm: bool,
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
        &cache(warm),
        r#"{"restore":{"outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#,
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
    let base = fixture("report-witness-base")?;
    base.git(&["add", "."]);
    assert!(super::cli(&base, &["inputs", "save"]).status.success());

    let before = base.fork("report-witness-before");
    before.write("src/lib.rs", "pub fn changed_before_restore() {}\n");
    assert!(super::cli(&before, &["inputs", "restore"]).status.success());
    let before_event = event(&before, "Compiling", "demo", "0.1.0", true);
    let prior_change = progress(&before, std::slice::from_ref(&before_event), true)?;
    assert_eq!(prior_change.verification_errors, [] as [String; 0]);
    assert!(prior_change.violations.is_empty(), "{prior_change:?}");

    let during = base.fork("report-witness-during");
    restore(&during);
    assert!(during.root().join(RESTORE_WITNESS).is_file());
    during.write("src/lib.rs", "pub fn changed() {}\n");
    let during_event = event(&during, "Compiling", "demo", "0.1.0", true);
    let changed = progress(&during, &[during_event], true)?;
    assert_ne!(changed.verification_errors, [] as [String; 0]);
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
    let cold_metrics = progress(&cold, &[dependency], false)?;
    assert_eq!(
        cold_metrics.verification_errors,
        [] as [String; 0],
        "{cold_metrics:?}"
    );

    assert!(super::cli(&warm, &["inputs", "save"]).status.success());
    assert!(super::cli(&warm, &["inputs", "restore"]).status.success());
    let dependency = event(&warm, "Compiling", "dependency", "1.0.0", false);
    let metrics = progress(&warm, &[dependency], true)?;
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("symlink")),
        "{metrics:?}"
    );
    let workspace = event(&warm, "Compiling", "demo", "0.1.0", true);
    let metrics = progress(&warm, &[workspace], true)?;
    assert_ne!(metrics.verification_errors, [] as [String; 0]);

    let quiet = artifact(
        &warm,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        false,
        false,
    )?;
    let metrics = observation(&warm, &[], &[quiet], true)?;
    assert_ne!(metrics.verification_errors, [] as [String; 0]);

    let fresh = artifact(
        &warm,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        true,
        false,
    )?;
    let metrics = observation(&warm, &[], &[fresh], true)?;
    assert_eq!(metrics.verification_errors, [] as [String; 0]);
    Ok(())
}

#[test]
fn quiet_artifacts_count_once_and_only_nonfresh_units_are_waste()
-> Result<(), Box<dyn std::error::Error>> {
    let base = fixture("report-quiet-artifacts-base")?;
    restore(&base);

    let quiet = base.fork("report-quiet-artifact");
    let lib = artifact(
        &quiet,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        false,
        false,
    )?;
    let metrics = observation(&quiet, &[], &[lib], true)?;
    assert_eq!(metrics.invocations[0].compilations.len(), 1, "{metrics:?}");
    assert!(
        metrics
            .violations
            .iter()
            .any(|line| line.contains("demo v0.1.0")),
        "{metrics:?}"
    );

    let fresh = base.fork("report-fresh-artifact");
    let lib = artifact(
        &fresh,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        true,
        false,
    )?;
    let metrics = observation(&fresh, &[], &[lib], true)?;
    assert!(metrics.invocations[0].compilations.is_empty());
    assert_eq!(metrics.violations, [] as [String; 0]);

    let progress_and_artifact = base.fork("report-progress-artifact");
    let lib = artifact(
        &progress_and_artifact,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        false,
        false,
    )?;
    let event = event(&progress_and_artifact, "Compiling", "demo", "0.1.0", true);
    let metrics = observation(&progress_and_artifact, &[event], &[lib], true)?;
    assert_eq!(metrics.invocations[0].compilations.len(), 1, "{metrics:?}");

    let both = base.fork("report-lib-test-artifacts");
    let lib = artifact(
        &both,
        "demo",
        "lib",
        "target/debug/deps/libdemo-hash.rmeta",
        false,
        false,
    )?;
    let test = artifact(
        &both,
        "demo",
        "test",
        "target/debug/deps/demo-test-hash",
        false,
        true,
    )?;
    let snapshot = both.root().join(SNAPSHOT);
    fs::remove_file(snapshot)?;
    let metrics = observation(&both, &[], &[lib, test], true)?;
    assert_eq!(metrics.invocations[0].compilations.len(), 1, "{metrics:?}");
    assert!(velnor_ci::report::check(&[metrics]).is_ok());
    Ok(())
}

#[test]
fn exact_source_downloads_require_a_changed_crate_archive() -> Result<(), Box<dyn std::error::Error>>
{
    let base = fixture("report-download-archive-base")?;
    let real = base.fork("report-download-real");
    let lookalike = base.fork("report-download-lookalike");
    let source_cache = json!([{"family":"sources","root":".","restore_step":"restore","save_step":"save","requested_key":"source-key","matched_key":"source-key","hit":"exact","bytes":null,"usable":false,"usable_same_lock":false,"save":"not_attempted"}]);
    let steps = r#"{"restore":{"outputs":{"cache-primary-key":"source-key","cache-matched-key":"source-key","cache-hit":"true"}}}"#;

    let real_home = cargo_home(&real)?;
    run_downloaded_cargo(&real, &real_home, true)?;
    let metrics = super::report(&real, &source_cache, steps)?;
    assert_eq!(
        metrics.invocations[0].verified_download_archives,
        Some(vec!["demo-0.1.0.crate".to_owned()])
    );
    assert_eq!(metrics.invocations[0].downloads, ["demo v0.1.0"]);
    assert!(
        metrics
            .violations
            .iter()
            .any(|line| line.contains("exact sources hit"))
    );

    let lookalike_home = cargo_home(&lookalike)?;
    run_downloaded_cargo(&lookalike, &lookalike_home, false)?;
    let metrics = super::report(&lookalike, &source_cache, steps)?;
    assert_eq!(
        metrics.invocations[0].verified_download_archives,
        Some(Vec::new())
    );
    assert_eq!(metrics.invocations[0].downloads, [] as [String; 0]);
    assert_eq!(metrics.violations, [] as [String; 0]);
    assert!(
        metrics
            .diagnostics
            .iter()
            .any(|line| line.contains("unverified download event"))
    );
    Ok(())
}

fn cargo_home(repo: &Repo) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let home = repo.root().join("cargo-home");
    fs::create_dir_all(&home)?;
    Ok(home)
}

fn run_downloaded_cargo(
    repo: &Repo,
    cargo_home: &std::path::Path,
    create_archive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let archive = cargo_home
        .join("registry/cache/index")
        .join("demo-0.1.0.crate");
    let script = repo.root().join("cargo-fixture");
    let create = if create_archive {
        format!(
            "mkdir -p '{}'\nprintf archive > '{}'\n",
            archive.parent().ok_or("archive parent")?.display(),
            archive.display()
        )
    } else {
        String::new()
    };
    fs::write(
        &script,
        format!("#!/bin/sh\nprintf '    Downloaded demo v0.1.0\\n' >&2\n{create}"),
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    repo.git(&["add", "."]);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--"])
        .arg(&script)
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("CARGO_HOME", cargo_home)
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}

fn read(repo: &Repo) -> LaneMetrics {
    serde_json::from_slice(&fs::read(repo.root().join("ci-metrics.json")).unwrap_or_default())
        .unwrap_or_else(|error| unreachable!("{error}"))
}
