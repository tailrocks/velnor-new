// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, ResultTest, fixture, read};
use std::process::Command;
use velnor_ci::report::LaneMetrics;
use velnor_ci::waste_evidence::MbxSession;

const SWITCH: &str = "VELNOR_MBX_REPORTS";

fn report_json(version: u64, hits: u64, misses: u64, unconsulted: u64) -> String {
    format!(
        r#"{{"version":{version},"session_duration_ns":27145500,"wrapper_phases_ns":{{}},"lookups":{},"hits":{hits},"misses":{misses},"unconsulted":{unconsulted},"bypasses":{{}},"downloaded_bytes":0,"uploaded_bytes":0,"stored_bytes":0,"remote_failures":0}}"#,
        hits + misses
    )
}

fn cargo_session(repo: &Repo, report: &str, stderr: &str) -> ResultTest {
    let cargo = repo.root().join("cargo-fixture");
    if !cargo.exists() {
        std::os::unix::fs::symlink("/bin/sh", &cargo)?;
    }
    let script = format!(
        "[ -n \"$MBX_STATS_REPORT\" ] && [ -n '{report}' ] && printf '%s' '{report}' > \"$MBX_STATS_REPORT\"; printf '%s\\n' '{stderr}' >&2; true"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", cargo.to_str().ok_or("path")?, "-c", &script])
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env(SWITCH, "true")
        .env_remove("MBX_STATS_REPORT")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}

fn report(repo: &Repo) -> Result<LaneMetrics, Box<dyn std::error::Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("report")
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_JOB", "rust")
        .env(SWITCH, "true")
        .env("VELNOR_REPORT_CACHES", "[]")
        .env("VELNOR_REPORT_STEPS", "{}")
        .env_remove("GITHUB_TOKEN")
        .env_remove("MBX_CACHE_DIR")
        .env_remove("MBX_STATS_REPORT")
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(read(repo))
}

const fn session(hits: u64, misses: u64, not_looked_up: u64) -> MbxSession {
    MbxSession {
        hits,
        misses,
        not_looked_up,
    }
}

#[test]
fn every_cargo_session_in_a_job_keeps_its_own_report() -> ResultTest {
    let repo = fixture("mbx-reports-two-sessions")?;
    cargo_session(&repo, &report_json(5, 0, 0, 1), "compiled")?;
    cargo_session(&repo, &report_json(5, 1, 0, 0), "compiled")?;
    cargo_session(&repo, &report_json(5, 0, 0, 0), "fresh")?;
    let metrics = report(&repo)?;
    assert_eq!(
        metrics.mbx.sessions,
        [session(0, 0, 1), session(1, 0, 0)],
        "{metrics:?}"
    );
    assert_eq!(metrics.mbx.unparsed, 0);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}

#[test]
fn a_report_of_an_unknown_version_is_not_understood() -> ResultTest {
    let repo = fixture("mbx-reports-unknown-version")?;
    cargo_session(&repo, &report_json(5, 2, 1, 0), "compiled")?;
    cargo_session(&repo, &report_json(6, 4, 0, 0), "compiled")?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(2, 1, 0)], "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1);
    assert!(
        metrics.verification_errors.iter().any(|line| line
            .contains("MBX stats report not understood (1 report(s))")
            && line.contains("version 6")),
        "{:?}",
        metrics.verification_errors
    );
    assert!(
        velnor_ci::report::check(std::slice::from_ref(&metrics)).is_err_and(|error| error
            .to_string()
            .contains("MBX stats report not understood"))
    );
    Ok(())
}

#[test]
fn reports_replace_the_text_summary_and_without_reports_the_text_counts() -> ResultTest {
    let line = "mbx[cache]: object cache: 0 hits, 7 misses; 0 B downloaded";
    let repo = fixture("mbx-reports-text-ignored")?;
    cargo_session(&repo, &report_json(5, 3, 0, 0), line)?;
    assert_eq!(report(&repo)?.mbx.sessions, [session(3, 0, 0)]);
    let repo = fixture("mbx-reports-text-fallback")?;
    cargo_session(&repo, "", line)?;
    assert_eq!(report(&repo)?.mbx.sessions, [session(0, 7, 0)]);
    Ok(())
}

#[test]
fn the_reader_maps_a_real_mbx_report_and_names_what_it_cannot_read() {
    use velnor_ci::mbx_report::parse;
    let real = include_bytes!("../data/mbx_report/mbx-1.30.0.json");
    assert_eq!(parse(real), Ok(Some(session(0, 0, 2))));
    let probes = br#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":2,"standard-input":1}}"#;
    assert_eq!(parse(probes), Ok(None));
    let bypassed = br#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":1,"other":1}}"#;
    assert_eq!(parse(bypassed), Ok(Some(session(0, 0, 0))));
    assert_eq!(parse(report_json(5, 0, 0, 0).as_bytes()), Ok(None));
    assert!(parse(br#"{"version":5,"hits":1}"#).is_err_and(|e| e.contains("without its counts")));
    assert!(parse(br#"{"hits":1}"#).is_err_and(|e| e.contains("no numeric version")));
    assert!(parse(b"{").is_err_and(|e| e.contains("not JSON")));
}
