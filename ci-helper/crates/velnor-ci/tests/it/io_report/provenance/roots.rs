// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, artifact, event, observation, progress, restore};
use serde_json::json;
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;
use velnor_ci::telemetry::INVOCATIONS;

type ResultTest = Result<(), Box<dyn std::error::Error>>;

#[test]
fn independent_symlink_root_preserves_regular_warm_dependency_waste() -> ResultTest {
    independent_roots(true)
}

#[test]
fn regular_nested_root_uses_its_own_restoration_witness() -> ResultTest {
    independent_roots(false)
}

fn independent_roots(linked: bool) -> ResultTest {
    let repo = Repo::new("report-independent-roots");
    for name in ["regular", "other"] {
        repo.write(
            &format!("{name}/Cargo.toml"),
            &format!("[package]\nname='{name}'\nversion='0.1.0'\nedition='2024'\n"),
        );
        repo.write(&format!("{name}/src/lib.rs"), "pub fn value() {}\n");
        repo.write(
            &format!("{name}/Cargo.lock"),
            &format!("version=4\n[[package]]\nname='{name}'\nversion='0.1.0'\n"),
        );
    }
    super::super::metadata_inputs(&repo)?;
    velnor_gen::io::write_metadata(repo.root())?;
    if linked {
        fs::remove_file(repo.root().join("other/Cargo.lock"))?;
        symlink(
            "/missing/external/Cargo.lock",
            repo.root().join("other/Cargo.lock"),
        )?;
    }
    repo.git(&["add", "."]);
    for name in ["regular", "other"] {
        for action in ["save", "restore"] {
            let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
                .args(["inputs", action])
                .current_dir(repo.root().join(name))
                .output()?;
            assert!(output.status.success(), "{output:?}");
        }
    }
    let root = fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root.join("regular"));
    invocation["compilations"] = json!([]);
    invocation["artifacts"] = json!([]);
    invocation["progress"] = json!([
        {"kind":"Compiling","package":"dependency","version":"1.0.0","location":null},
        {"kind":"Checking","package":"regular","version":"0.1.0","location":root.join("regular")}
    ]);
    fs::write(repo.root().join(INVOCATIONS), format!("{invocation}\n"))?;
    let caches =
        json!([{"family":"target","root":"regular","restore_step":"restore","save_step":"save"}]);
    let metrics = super::super::report(
        &repo,
        &caches,
        r#"{"restore":{"outputs":{"cache-primary-key":"key","cache-matched-key":"key","cache-hit":"true"}}}"#,
    )?;
    assert_eq!(metrics.invocations[0].root, "regular");
    assert_eq!(metrics.invocations[0].compilations.len(), 2);
    assert!(metrics.caches[0].usable_same_lock);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    for package in ["dependency", "regular"] {
        assert!(
            metrics.violations.iter().any(|line| line.contains(package)),
            "{metrics:?}"
        );
    }
    Ok(())
}

#[test]
fn dangling_lock_keeps_cold_progress_quiet_artifacts_and_third_compile_enforcement() -> ResultTest {
    let repo = super::super::fixture("report-dangling-lock-telemetry")?;
    fs::remove_file(repo.root().join("Cargo.lock"))?;
    symlink(
        "/missing/external/Cargo.lock",
        repo.root().join("Cargo.lock"),
    )?;
    repo.git(&["add", "."]);
    let events = [event(&repo, "Compiling", "dependency", "1.0.0", false)];
    let metrics = progress(&repo, &events)?;
    assert_eq!(metrics.invocations[0].root, ".");
    assert_eq!(metrics.invocations[0].compilations.len(), 1);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    let quiet = artifact(
        &repo,
        "demo",
        "lib",
        "target/debug/deps/libdemo.rmeta",
        false,
        false,
    )?;
    let metrics = observation(&repo, &[], std::slice::from_ref(&quiet))?;
    assert_eq!(metrics.invocations[0].compilations.len(), 1);
    assert_eq!(
        metrics.invocations[0].artifacts[0].manifest_path,
        "checkout/Cargo.toml"
    );
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    let repeated = vec![events[0].clone(); 3];
    let metrics = progress(&repo, &repeated)?;
    assert_eq!(metrics.invocations[0].compilations.len(), 3);
    let error = velnor_ci::report::check(&[metrics])
        .err()
        .ok_or("third compile must fail")?;
    assert!(error.to_string().contains("compiled 3 times"), "{error}");
    restore(&repo);
    let metrics = observation(&repo, &[], &[quiet])?;
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("Cargo.lock symlink")),
        "{metrics:?}"
    );
    assert_eq!(metrics.invocations[0].compilations.len(), 1);
    Ok(())
}
