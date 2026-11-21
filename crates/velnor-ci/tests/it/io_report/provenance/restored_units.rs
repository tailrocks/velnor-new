// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use velnor_ci::inputs::restored_units_file;
use velnor_ci::restored_units::Manifest;

mod missing_input;

const HELD: &str = "0123456789abcdef";
const HELD_NEW_LAYOUT: &str = "fedcba9876543210";
const GAP: &str = "00000000deadbeef";

fn base(name: &str, layout: &str) -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = fixture(name)?;
    let (path, held) = match layout {
        "old" => (
            format!("target/debug/.fingerprint/serde-{HELD}/lib-serde"),
            HELD,
        ),
        _ => (
            format!("target/debug/build/serde/{HELD_NEW_LAYOUT}/fingerprint/lib-serde"),
            HELD_NEW_LAYOUT,
        ),
    };
    repo.write(&path, "{}\n");
    restore(&repo);
    let record = fs::read_to_string(repo.root().join("target").join(restored_units_file("")))?;
    assert!(record.lines().any(|line| line == held), "{record}");
    Ok(repo)
}

fn registry_artifact(repo: &Repo, hash: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let mut value = artifact(
        repo,
        "serde",
        "lib",
        &format!("target/debug/deps/libserde-{hash}.rmeta"),
        false,
        false,
    )?;
    value["package_id"] =
        json!("registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0");
    Ok(value)
}

#[test]
fn a_registry_unit_is_waste_only_when_the_restored_base_held_it_in_either_layout()
-> Result<(), Box<dyn std::error::Error>> {
    for (layout, held) in [("old", HELD), ("new", HELD_NEW_LAYOUT)] {
        let repo = base(&format!("report-restored-units-{layout}"), layout)?;
        let rebuilt = registry_artifact(&repo, held)?;
        let metrics = observation(&repo, &[], &[rebuilt])?;
        assert_eq!(
            metrics.invocations[0].restored_manifest,
            Some(Manifest::Present)
        );
        assert_eq!(metrics.invocations[0].compilations[0].restored, Some(true));
        assert!(
            metrics
                .violations
                .iter()
                .any(|line| line.contains("serde v1.0.0")),
            "{layout}: {metrics:?}"
        );

        let gap = repo.fork(&format!("report-restored-units-gap-{layout}"));
        let missing = registry_artifact(&gap, GAP)?;
        let metrics = observation(&gap, &[], &[missing])?;
        assert_eq!(metrics.invocations[0].compilations[0].restored, Some(false));
        assert_eq!(metrics.violations, [] as [String; 0], "{layout}");
    }
    Ok(())
}

#[test]
fn a_restore_that_left_no_record_is_unknown_not_waste_and_says_so()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = base("report-restored-units-absent", "old")?;
    fs::remove_file(repo.root().join("target").join(restored_units_file("")))?;
    let missing = registry_artifact(&repo, GAP)?;
    let metrics = observation(&repo, &[], &[missing])?;
    assert_eq!(metrics.invocations[0].compilations[0].restored, None);
    assert_eq!(
        metrics.invocations[0].restored_manifest,
        Some(Manifest::Absent)
    );
    assert_eq!(metrics.violations, [] as [String; 0]);
    let report = check(&repo);
    assert!(report.status.success(), "{report:?}");
    assert!(
        String::from_utf8_lossy(&report.stdout).contains(
            "::warning::.: dependency compiles with an unknown restore status (restored-units manifest absent): serde v1.0.0"
        ),
        "{report:?}"
    );
    Ok(())
}

#[test]
fn a_record_that_cannot_be_decoded_is_unreadable_not_absent()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = base("report-restored-units-garbage", "old")?;
    fs::write(
        repo.root().join("target").join(restored_units_file("")),
        "not a record\n",
    )?;
    let missing = registry_artifact(&repo, GAP)?;
    let metrics = observation(&repo, &[], &[missing])?;
    assert_eq!(metrics.invocations[0].compilations[0].restored, None);
    assert_eq!(
        metrics.invocations[0].restored_manifest,
        Some(Manifest::Unreadable)
    );
    assert_eq!(metrics.violations, [] as [String; 0]);
    Ok(())
}

#[test]
fn a_restore_whose_scan_fails_clears_the_older_record_so_no_unit_is_claimed_as_held()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = base("report-restored-units-stale", "old")?;
    let record = repo.root().join("target").join(restored_units_file(""));
    assert!(record.is_file());
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", "restore"])
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env("VELNOR_TESTS_RUN", "7")
        .env("VELNOR_TARGET_MAX_DIRECTORIES", "1")
        .env_remove("GITHUB_TOKEN")
        .env_remove("CARGO_TARGET_DIR")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    assert!(
        !record.exists(),
        "an older record survived a restore whose scan failed"
    );
    let gap = registry_artifact(&repo, GAP)?;
    let metrics = observation(&repo, &[], &[gap])?;
    assert_eq!(metrics.invocations[0].compilations[0].restored, None);
    assert_eq!(
        metrics.invocations[0].restored_manifest,
        Some(Manifest::Absent)
    );
    assert_eq!(metrics.violations, [] as [String; 0]);
    Ok(())
}

#[test]
fn cargos_dirty_reason_reaches_the_compilation_it_explains()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = base("report-restored-units-dirty", "old")?;
    let missing = registry_artifact(&repo, GAP)?;
    let reason = "FsStatusOutdated(StaleItem(MissingFile { path: \"/store/out-dirs/v1/x.rs\" }))";
    let metrics = observation_with(
        &repo,
        &[],
        &[missing],
        json!([
            {"package":"serde","version":"1.0.0","reason":"FsStatusOutdated(StaleDepFingerprint { unit: UnitIndex(1) })"},
            {"package":"serde","version":"1.0.0","reason":reason},
            {"package":"other","version":"2.0.0","reason":"ignored"}
        ]),
    )?;
    assert_eq!(
        metrics.invocations[0].compilations[0].dirty.as_deref(),
        Some(reason)
    );
    Ok(())
}

