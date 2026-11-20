// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mbx_reports::{SWITCH, cargo, cargo_with, report, report_json, session};
use super::{Repo, ResultTest, fixture};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use velnor_ci::mbx_report::reports_session;

fn plant(repo: &Repo, name: &str) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let directory = repo.root().join("velnor-mbx-reports");
    fs::create_dir_all(&directory)?;
    Ok(directory.join(name))
}

#[test]
fn a_planted_symlink_or_oversized_report_is_unreadable_evidence_not_read() -> ResultTest {
    let repo = fixture("mbx-reports-planted")?;
    cargo(&repo, "build", &report_json(5, 1, 0, 0), "compiled")?;
    let real = repo.root().join("real.json");
    fs::write(&real, report_json(5, 7, 0, 0))?;
    std::os::unix::fs::symlink(&real, plant(&repo, "1-linked.json")?)?;
    fs::write(plant(&repo, "2-huge.json")?, vec![b' '; 2 << 20])?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(1, 0, 0)], "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 2);
    let errors = metrics.verification_errors.join("\n");
    assert!(
        errors.contains("(2 report(s)); first: 1-linked.json: not a regular file"),
        "{errors}"
    );
    fs::remove_file(plant(&repo, "1-linked.json")?)?;
    let errors = report(&repo)?.verification_errors.join("\n");
    assert!(errors.contains("2-huge.json: larger than"), "{errors}");
    Ok(())
}

fn plant_session(repo: &Repo, name: &str, json: Option<&str>) -> ResultTest {
    fs::write(plant(repo, &format!("{name}.expected"))?, b"")?;
    if let Some(json) = json {
        fs::write(plant(repo, &format!("{name}.json"))?, json)?;
    }
    Ok(())
}

#[test]
fn three_hundred_sessions_in_one_job_are_all_read() -> ResultTest {
    let repo = fixture("mbx-reports-three-hundred")?;
    for index in 0..300 {
        plant_session(
            &repo,
            &format!("{index:05}"),
            Some(&report_json(5, 1, 0, 0)),
        )?;
    }
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions.len(), 300, "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}

#[test]
fn four_thousand_ninety_six_sessions_are_the_largest_read_in_full() -> ResultTest {
    let repo = fixture("mbx-reports-cap")?;
    for index in 0..4096 {
        plant_session(
            &repo,
            &format!("{index:05}"),
            Some(&report_json(5, 1, 0, 0)),
        )?;
    }
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions.len(), 4096);
    assert_eq!(metrics.mbx.unparsed, 0);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}

#[test]
fn a_job_past_4096_sessions_is_unknown_evidence_that_fails_the_check() -> ResultTest {
    let repo = fixture("mbx-reports-flood")?;
    for index in 0..4097 {
        plant_session(
            &repo,
            &format!("{index:05}"),
            Some(&report_json(5, 1, 0, 0)),
        )?;
    }
    let metrics = report(&repo)?;
    assert!(
        metrics.mbx.sessions.is_empty(),
        "{}",
        metrics.mbx.sessions.len()
    );
    assert_eq!(metrics.mbx.unparsed, 1);
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("MBX stats reports exceed")),
        "{:?}",
        metrics.verification_errors
    );
    assert!(velnor_ci::report::check(std::slice::from_ref(&metrics)).is_err());
    Ok(())
}

#[test]
fn a_dangling_json_symlink_is_one_unreadable_report_not_also_missing() -> ResultTest {
    let repo = fixture("mbx-reports-dangling")?;
    cargo(&repo, "build", &report_json(5, 1, 0, 0), "compiled")?;
    plant_session(&repo, "5-dangling", None)?;
    std::os::unix::fs::symlink(
        repo.root().join("nowhere.json"),
        plant(&repo, "5-dangling.json")?,
    )?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(1, 0, 0)], "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    let errors = metrics.verification_errors.join("\n");
    assert!(
        errors.contains("(1 report(s)); first: 5-dangling.json: not a regular file"),
        "{errors}"
    );
    assert!(!errors.contains("missing"), "{errors}");
    Ok(())
}

#[test]
fn a_session_inside_an_enclosing_mbx_session_gets_no_report_path() -> ResultTest {
    let repo = fixture("mbx-reports-nested")?;
    cargo_with(
        &repo,
        "build",
        &report_json(5, 1, 0, 0),
        "compiled",
        None,
        true,
        true,
    )?;
    assert!(!repo.root().join("velnor-mbx-reports").exists());
    let metrics = report(&repo)?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}

#[test]
fn a_session_whose_expected_report_cannot_be_recorded_still_runs_and_counts_as_unparsed()
-> ResultTest {
    let repo = fixture("mbx-reports-unrecordable")?;
    cargo(&repo, "build", &report_json(5, 1, 0, 0), "compiled")?;
    let directory = repo.root().join("velnor-mbx-reports");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555))?;
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([
            "run",
            "--",
            repo.root().join("fake-bin/cargo").to_str().ok_or("path")?,
            "build",
        ])
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env(SWITCH, "true")
        .env("FAKE_MBX_REPORT", report_json(5, 7, 0, 0))
        .env("FAKE_MBX_STDERR", "compiled")
        .env_remove("MBX_STATS_REPORT")
        .env_remove("MBX_SOCKET")
        .output()?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("cannot record the expected MBX report"),
        "{stderr}"
    );
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(1, 0, 0)], "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("MBX stats could not be recorded for 1 session(s)")),
        "{:?}",
        metrics.verification_errors
    );
    Ok(())
}

#[test]
fn an_unwritable_stats_directory_with_no_other_session_is_unparsed_not_silent() -> ResultTest {
    let repo = fixture("mbx-reports-unwritable")?;
    let directory = repo.root().join("velnor-mbx-reports");
    fs::create_dir_all(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555))?;
    let ran = cargo(&repo, "build", &report_json(5, 1, 0, 0), "compiled");
    let metrics = report(&repo);
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;
    ran?;
    let metrics = metrics?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("MBX stats could not be recorded for 1 session(s)")),
        "{:?}",
        metrics.verification_errors
    );
    Ok(())
}

#[test]
fn a_report_without_its_expected_marker_is_unreadable_evidence() -> ResultTest {
    let repo = fixture("mbx-reports-unmarked")?;
    cargo(&repo, "build", &report_json(5, 1, 0, 0), "compiled")?;
    fs::write(plant(&repo, "0-planted.json")?, report_json(5, 5, 0, 0))?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(1, 0, 0)], "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1);
    let errors = metrics.verification_errors.join("\n");
    assert!(
        errors.contains("0-planted.json: no expected marker"),
        "{errors}"
    );
    Ok(())
}

#[test]
fn a_nextest_listing_builds_test_binaries_so_it_gets_a_report() {
    let argv = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<String>>()
    };
    assert!(reports_session(&argv(&[
        "cargo",
        "nextest",
        "list",
        "--workspace"
    ])));
    assert!(reports_session(&argv(&[
        "cargo-nextest",
        "nextest",
        "list",
        "--workspace"
    ])));
    assert!(!reports_session(&argv(&[
        "cargo",
        "nextest",
        "run",
        "--workspace"
    ])));
}
