// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{RESTORE_WITNESS, Repo, SNAPSHOT};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::process::Command;
use velnor_ci::inputs::INPUTS_FILE;
use velnor_ci::report::LaneMetrics;
use velnor_ci::telemetry::INVOCATIONS;

type ResultTest = Result<(), Box<dyn std::error::Error>>;
const HIT: &str = r#"{"restore":{"outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#;

fn fixture(root: &str) -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = Repo::new("report-configured-target");
    let path = |name: &str| format!("{root}{name}");
    repo.write_all(&[
        (".gitignore", "/target/\n/cache-output/\n/nested/target/\n"),
        (
            &path("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
        ),
        (&path("src/lib.rs"), "pub fn value() {}\n"),
        (
            &path("Cargo.lock"),
            "version=4\n[[package]]\nname='demo'\nversion='0.1.0'\n",
        ),
        ("sibling.txt", "outside Cargo root\n"),
    ]);
    let target = if root.is_empty() {
        "cache-output"
    } else {
        "../cache-output"
    };
    repo.write(
        &path(".cargo/config.toml"),
        &format!("[build]\ntarget-dir='{target}'\n"),
    );
    super::super::metadata_inputs(&repo)?;
    velnor_gen::io::write_metadata(repo.root())?;
    Ok(repo)
}

fn inputs(repo: &Repo, root: &str, action: &str) -> ResultTest {
    let target = if root.is_empty() {
        "cache-output"
    } else {
        "../cache-output"
    };
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", action, "--target-dir", target])
        .current_dir(repo.root().join(root))
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}

fn report(repo: &Repo, root: &str, steps: &str) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let directory = fs::canonicalize(repo.root().join(root))?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["argv"] = json!(["cargo", "check"]);
    invocation["root"] = json!(directory);
    invocation["compilations"] = json!([]);
    invocation["progress"] = json!([
        {"kind":"Compiling","package":"dependency","version":"1.0.0","location":null},
        {"kind":"Checking","package":"demo","version":"0.1.0","location":directory}
    ]);
    fs::write(repo.root().join(INVOCATIONS), format!("{invocation}\n"))?;
    let root = if root.is_empty() {
        "."
    } else {
        root.trim_end_matches('/')
    };
    let cache =
        json!([{"family":"target","root":root,"restore_step":"restore","save_step":"save"}]);
    super::super::report(repo, &cache, steps)
}

#[test]
fn configured_target_warm_report_ignores_default_target_decoy() -> ResultTest {
    let repo = fixture("")?;
    inputs(&repo, "", "save")?;
    inputs(&repo, "", "restore")?;
    fs::create_dir_all(repo.root().join("target/.velnor"))?;
    fs::write(repo.root().join("target").join(SNAPSHOT), "malformed\n")?;
    fs::write(repo.root().join("target").join(RESTORE_WITNESS), "stale\n")?;
    let metrics = report(&repo, "", HIT)?;
    assert!(metrics.caches[0].usable, "{metrics:?}");
    assert!(metrics.caches[0].usable_same_lock, "{metrics:?}");
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    for package in ["dependency v1.0.0", "demo v0.1.0"] {
        assert!(
            metrics.violations.iter().any(|line| line.contains(package)),
            "{metrics:?}"
        );
    }
    assert_eq!(metrics.invocations[0].target_directory, None);
    Ok(())
}

#[test]
fn configured_target_restore_miss_rejects_stale_snapshot() -> ResultTest {
    let repo = fixture("")?;
    inputs(&repo, "", "save")?;
    inputs(&repo, "", "restore")?;
    let target = repo.root().join("cache-output");
    assert!(target.join(SNAPSHOT).is_file());
    let metrics = report(&repo, "", "{}")?;
    assert!(!metrics.caches[0].usable, "{metrics:?}");
    assert!(!metrics.caches[0].usable_same_lock, "{metrics:?}");
    assert!(metrics.violations.is_empty(), "{metrics:?}");
    fs::remove_file(target.join(INPUTS_FILE))?;
    inputs(&repo, "", "restore")?;
    assert!(!target.join(SNAPSHOT).exists());
    assert!(!target.join(RESTORE_WITNESS).exists());
    let metrics = report(&repo, "", HIT)?;
    assert!(!metrics.caches[0].usable, "{metrics:?}");
    assert!(metrics.violations.is_empty(), "{metrics:?}");
    Ok(())
}

#[test]
fn nested_configured_target_keeps_root_scope_and_pre_cargo_witness() -> ResultTest {
    let repo = fixture("nested/")?;
    inputs(&repo, "nested/", "save")?;
    inputs(&repo, "nested/", "restore")?;
    let target = repo.root().join("cache-output");
    let snapshot = fs::read(target.join(SNAPSHOT))?;
    let witness = fs::read(target.join(RESTORE_WITNESS))?;
    repo.write("sibling.txt", "outside root changed during Cargo\n");
    let metrics = report(&repo, "nested/", HIT)?;
    assert_eq!(metrics.invocations[0].root, "nested");
    assert!(metrics.caches[0].usable_same_lock, "{metrics:?}");
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    assert!(
        metrics.invocations[0]
            .compilations
            .iter()
            .any(|unit| unit.workspace && unit.unchanged),
        "{metrics:?}"
    );
    repo.write("nested/src/lib.rs", "pub fn changed_during_cargo() {}\n");
    inputs(&repo, "nested/", "save")?;
    assert_eq!(fs::read(target.join(SNAPSHOT))?, snapshot);
    assert_eq!(fs::read(target.join(RESTORE_WITNESS))?, witness);
    let metrics = report(&repo, "nested/", HIT)?;
    assert!(metrics.caches[0].usable_same_lock, "{metrics:?}");
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("changed during Cargo")),
        "{metrics:?}"
    );
    assert!(!repo.root().join("nested/target").join(SNAPSHOT).exists());
    assert!(!Path::new(&metrics.invocations[0].root).is_absolute());
    Ok(())
}
