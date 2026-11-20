// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mbx_reports::{cargo, cargo_with, report, report_json, session};
use super::{Repo, ResultTest, fixture};
use std::fs;

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
fn a_job_past_4096_sessions_reports_its_stats_unavailable_without_failing() -> ResultTest {
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
        metrics.verification_errors.is_empty(),
        "{:?}",
        metrics.verification_errors
    );
    assert!(velnor_ci::report::check(std::slice::from_ref(&metrics)).is_ok());
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
    let socket = repo.root().join("cache-agent.sock");
    let _daemon = std::os::unix::net::UnixListener::bind(&socket)?;
    cargo_with(
        &repo,
        "build",
        &report_json(5, 1, 0, 0),
        "compiled",
        None,
        true,
        Some(&socket),
    )?;
    assert!(!repo.root().join("velnor-mbx-reports").exists());
    let metrics = report(&repo)?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}
