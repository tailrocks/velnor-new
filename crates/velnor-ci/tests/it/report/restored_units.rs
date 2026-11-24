// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn with_restored(restored: Option<bool>) -> LaneMetrics {
    let mut value = lane();
    if let Some(restored) = restored {
        value["invocations"][0]["compilations"][0]["restored"] = json!(restored);
    }
    metrics(value)
}

#[test]
fn a_dependency_unit_the_restored_base_held_is_still_waste() {
    assert_eq!(
        waste(&with_restored(Some(true))),
        [".: serde v1.0.0 compiled despite usable restored inputs"]
    );
}

#[test]
fn a_dependency_unit_the_restored_base_never_held_is_a_notice_not_waste() {
    let lane = with_restored(Some(false));
    assert_eq!(waste(&lane), [] as [String; 0]);
    let report = check(&[lane]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(report.contains("no waste"), "{report}");
    assert!(
        report.contains(
            ".: dependency packages whose compiled units were absent from the restored base (the run that saved it built a different package or feature selection): serde v1.0.0"
        ),
        "{report}"
    );
}

#[test]
fn a_dependency_unit_with_no_restore_status_is_unknown_not_waste() {
    let lane = with_restored(None);
    assert_eq!(waste(&lane), [] as [String; 0]);
    let report = check(&[lane]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(report.contains("no waste"), "{report}");
    assert!(
        report.contains(
            "::warning::.: dependency compiles with an unknown restore status (restored-units manifest not recorded): serde v1.0.0"
        ),
        "{report}"
    );
}

#[test]
fn a_missing_or_unreadable_manifest_is_a_warning_and_a_present_one_a_notice() {
    for (state, level) in [
        ("absent", "warning"),
        ("unreadable", "warning"),
        ("present", "notice"),
    ] {
        let mut value = lane();
        value["invocations"][0]["restored_manifest"] = json!(state);
        let report = check(&[metrics(value)]).unwrap_or_else(|error| unreachable!("{error}"));
        assert!(
            report.contains(&format!(
                "::{level}::.: dependency compiles with an unknown restore status (restored-units manifest {state}): serde v1.0.0"
            )),
            "{state}: {report}"
        );
    }
}

#[test]
fn a_failing_check_keeps_the_restore_warning_out_of_the_error_text() {
    let mut value = lane();
    value["invocations"][0]["restored_manifest"] = json!("absent");
    value["violations"] = json!([".: a failing lane"]);
    let lanes = [metrics(value)];
    let error = check(&lanes)
        .expect_err("a violation fails the check")
        .to_string();
    assert!(error.contains(".: a failing lane"), "{error}");
    assert!(!error.contains("::warning::"), "{error}");
    let notices = restore_notices(&lanes);
    assert!(
        notices.contains(
            "::warning::.: dependency compiles with an unknown restore status (restored-units manifest absent): serde v1.0.0\n"
        ),
        "{notices}"
    );
}

#[test]
fn cargos_dirty_reason_is_named_for_an_unknown_and_for_waste() {
    let reason = "FsStatusOutdated(StaleItem(MissingFile { path: \"/store/out-dirs/v1/x.rs\" }))";
    let mut unknown = lane();
    unknown["invocations"][0]["compilations"][0]["dirty"] = json!(reason);
    let report = check(&[metrics(unknown)]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(
        report.contains(&format!("serde v1.0.0 (cargo: {reason})")),
        "{report}"
    );
    let mut held = lane();
    held["invocations"][0]["compilations"][0]["restored"] = json!(true);
    held["invocations"][0]["compilations"][0]["dirty"] = json!(reason);
    assert_eq!(
        waste(&metrics(held)),
        [format!(
            ".: serde v1.0.0 compiled despite usable restored inputs (cargo: {reason})"
        )]
    );
}

#[test]
fn a_manifest_state_round_trips_through_the_metrics() {
    for state in ["present", "absent", "unreadable"] {
        let mut value = lane();
        value["invocations"][0]["restored_manifest"] = json!(state);
        let text =
            serde_json::to_value(metrics(value)).unwrap_or_else(|error| unreachable!("{error}"));
        assert_eq!(text["invocations"][0]["restored_manifest"], json!(state));
    }
}

#[test]
fn a_gap_is_no_notice_on_a_cold_base() {
    let mut value = lane();
    value["caches"][0]["usable"] = json!(false);
    value["caches"][0]["usable_same_lock"] = json!(false);
    value["invocations"][0]["restored_target_usable"] = json!(false);
    value["invocations"][0]["compilations"][0]["restored"] = json!(false);
    let report = check(&[metrics(value)]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(!report.contains("restored base"), "{report}");
}

#[test]
fn a_record_that_is_not_ours_or_is_damaged_reads_as_absent() {
    use std::collections::BTreeSet;
    use velnor_ci::restored_units::{RestoredUnits, decode, encode};
    let hashes = |list: [&str; 2]| -> BTreeSet<String> { list.map(str::to_owned).into() };
    let units = RestoredUnits {
        present: hashes(["0123456789abcdef", "fedcba9876543210"]),
        missing_input: hashes(["00000000deadbeef", "00000000cafebabe"]),
    };
    assert_eq!(decode(&encode(&units)), Some(units));
    assert_eq!(
        decode(&encode(&RestoredUnits::default())),
        Some(RestoredUnits::default())
    );
    for text in [
        "",
        "other 2\n0123456789abcdef\n",
        "velnor-restored-units 1\n0123456789abcdef\n",
        "velnor-restored-units 2\nnot-a-hash\n",
        "velnor-restored-units 2\nmissing-input not-a-hash\n",
    ] {
        assert_eq!(decode(text), None, "{text:?}");
    }
}

#[test]
fn a_dependency_unit_whose_dep_info_names_a_missing_file_is_a_base_gap_with_that_reason() {
    let mut value = lane();
    value["invocations"][0]["compilations"][0]["restored"] = json!(false);
    value["invocations"][0]["compilations"][0]["missing_input"] = json!(true);
    let lane = metrics(value);
    assert_eq!(waste(&lane), [] as [String; 0]);
    let report = check(&[lane]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(report.contains("no waste"), "{report}");
    assert!(
        report.contains("dependency packages whose restored dep-info names a file outside target/ that is missing"),
        "{report}"
    );
    assert!(report.contains("MBX_SHARE_OUT_DIR=0"), "{report}");
    assert!(report.contains("serde v1.0.0"), "{report}");
    assert!(
        !report.contains("absent from the restored base"),
        "{report}"
    );
}

#[test]
fn a_gap_on_a_changed_lock_is_no_notice() {
    let mut value = lane();
    value["caches"][0]["usable_same_lock"] = json!(false);
    value["invocations"][0]["compilations"][0]["restored"] = json!(false);
    let report = check(&[metrics(value)]).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(!report.contains("restored base"), "{report}");
}

#[test]
fn a_failed_check_prints_the_restore_notices_on_their_own_lines() {
    let mut value = lane();
    value["invocations"][0]["compilations"][0]["restored"] = json!(false);
    value["invocations"][0]["compilations"][0]["missing_input"] = json!(true);
    value["violations"] = json!(["rust: a lane violation"]);
    let error = check(&[metrics(value.clone())])
        .err()
        .map_or_default(|error| error.to_string());
    assert_eq!(error, "rust: a lane violation");
    let notices = restore_notices(&[metrics(value)]);
    assert!(notices.starts_with("::notice::"), "{notices}");
    assert!(
        notices.contains("dependency packages whose restored dep-info names a file outside target/ that is missing"),
        "{notices}"
    );
}

#[test]
fn the_check_command_prints_the_restore_notices_before_it_fails()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = crate::io::Repo::empty("report-check-restore-notice");
    let mut value = lane();
    value["invocations"][0]["compilations"] = json!([
        {"package":"serde","version":"1.0.0","workspace":false,"unchanged":false,"dependencies":[],"restored":true},
        {"package":"tokio","version":"1.0.0","workspace":false,"unchanged":false,"dependencies":[],"restored":false,"missing_input":true}
    ]);
    std::fs::write(repo.root().join("ci-metrics.json"), value.to_string())?;
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["report", "--check"])
        .arg(repo.root())
        .env("GITHUB_REF", "refs/heads/main")
        .env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
        .env("VELNOR_REPORT_NEEDS", "{}")
        .env("GITHUB_STEP_SUMMARY", repo.root().join("summary"))
        .env_remove("GITHUB_TOKEN")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(
        stderr.contains("compiled despite usable restored inputs"),
        "{stderr}"
    );
    assert!(
        stdout.lines().any(|line| line.starts_with("::notice::")
            && line.contains("names a file outside target/ that is missing")
            && line.contains("tokio")),
        "{stdout}"
    );
    Ok(())
}
