// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Repo, ResultTest, fixture, read};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use velnor_ci::mbx_report::{parse, reports_session};
use velnor_ci::report::LaneMetrics;
use velnor_ci::waste_evidence::MbxSession;

const SWITCH: &str = "VELNOR_MBX_REPORTS";
const FAKE_CARGO: &str = "#!/bin/sh\nprintf '%s\\n' \"$FAKE_MBX_STDERR\" >&2\nif [ -n \"$MBX_STATS_REPORT\" ] && [ -n \"$FAKE_MBX_REPORT\" ]; then printf '%s' \"$FAKE_MBX_REPORT\" > \"$MBX_STATS_REPORT\"; fi\n";

fn report_json(version: u64, hits: u64, misses: u64, unconsulted: u64) -> String {
    format!(
        r#"{{"version":{version},"session_duration_ns":27145500,"wrapper_phases_ns":{{}},"lookups":{},"hits":{hits},"misses":{misses},"unconsulted":{unconsulted},"bypasses":{{}},"downloaded_bytes":0,"uploaded_bytes":0,"stored_bytes":0,"remote_failures":0}}"#,
        hits + misses
    )
}

fn cargo(repo: &Repo, subcommand: &str, report: &str, stderr: &str) -> ResultTest {
    cargo_with(repo, subcommand, report, stderr, None)
}

fn cargo_with(
    repo: &Repo,
    subcommand: &str,
    report: &str,
    stderr: &str,
    inherited: Option<&std::path::Path>,
) -> ResultTest {
    let bin = repo.root().join("fake-bin");
    let cargo = bin.join("cargo");
    if !cargo.exists() {
        fs::create_dir_all(&bin)?;
        fs::write(&cargo, FAKE_CARGO)?;
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))?;
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["run", "--", cargo.to_str().ok_or("path")?, subcommand])
        .current_dir(repo.root())
        .env("RUNNER_TEMP", repo.root())
        .env(SWITCH, "true")
        .env("FAKE_MBX_REPORT", report)
        .env("FAKE_MBX_STDERR", stderr)
        .env_remove("MBX_STATS_REPORT");
    if let Some(path) = inherited {
        command.env("MBX_STATS_REPORT", path);
    }
    let output = command.output()?;
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
    cargo(&repo, "build", &report_json(5, 0, 0, 1), "compiled")?;
    cargo(&repo, "check", &report_json(5, 1, 0, 0), "compiled")?;
    cargo(&repo, "build", &report_json(5, 0, 0, 0), "fresh")?;
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
    cargo(&repo, "build", &report_json(5, 2, 1, 0), "compiled")?;
    cargo(&repo, "build", &report_json(6, 4, 0, 0), "compiled")?;
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
fn a_command_that_runs_tests_gets_no_report_path_and_counts_from_its_summary() -> ResultTest {
    let line = "mbx[cache]: object cache: 0 hits, 7 misses; 0 B downloaded";
    let repo = fixture("mbx-reports-test-session")?;
    let inherited = repo.root().join("outer-report.json");
    cargo(&repo, "build", &report_json(5, 3, 0, 0), "reworded summary")?;
    cargo_with(
        &repo,
        "test",
        &report_json(5, 9, 9, 9),
        line,
        Some(&inherited),
    )?;
    assert!(!inherited.exists());
    let metrics = report(&repo)?;
    assert_eq!(
        metrics.mbx.sessions,
        [session(3, 0, 0), session(0, 7, 0)],
        "{metrics:?}"
    );
    assert_eq!(metrics.mbx.unparsed, 0);
    let repo = fixture("mbx-reports-text-fallback")?;
    cargo(&repo, "nextest", "", line)?;
    assert_eq!(report(&repo)?.mbx.sessions, [session(0, 7, 0)]);
    Ok(())
}

#[test]
fn a_build_that_printed_a_summary_but_wrote_no_report_is_unknown() -> ResultTest {
    let repo = fixture("mbx-reports-missing")?;
    cargo(
        &repo,
        "build",
        &report_json(5, 1, 0, 0),
        "mbx[cache]: object cache: 1 hits, 0 misses; 0 B downloaded",
    )?;
    cargo(
        &repo,
        "build",
        "",
        "mbx[cache]: object cache: 4 hits, 0 misses; 0 B downloaded",
    )?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(1, 0, 0)], "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1);
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("MBX stats report missing for 1 session(s)")),
        "{:?}",
        metrics.verification_errors
    );
    Ok(())
}

#[test]
fn only_commands_that_run_no_built_code_get_a_report() {
    let argv = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    for build in [
        &["cargo", "build", "--workspace"][..],
        &["/usr/bin/cargo", "+nightly", "check"],
        &["cargo-clippy", "clippy", "--all-targets"],
        &["cargo", "doc"],
    ] {
        assert!(reports_session(&argv(build)), "{build:?}");
    }
    for runs in [
        &["cargo", "test"][..],
        &["cargo", "nextest", "run"],
        &["cargo-nextest", "nextest", "run"],
        &["cargo", "run"],
        &["cargo", "--config", "x", "build"],
        &["velnor-ci", "inputs", "build"],
        &[],
    ] {
        assert!(!reports_session(&argv(runs)), "{runs:?}");
    }
}

#[test]
fn the_reader_maps_a_real_mbx_report_and_names_what_it_cannot_read() {
    let real = include_bytes!("../data/mbx_report/mbx-1.30.0.json");
    assert_eq!(parse(real), Ok(Some(session(0, 0, 2))));
    let probes = br#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":2,"standard-input":1}}"#;
    assert_eq!(parse(probes), Ok(None));
    let bypassed = br#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":1,"other":1}}"#;
    assert_eq!(parse(bypassed), Ok(Some(session(0, 0, 0))));
    let huge = format!(
        r#"{{"version":5,"hits":{max},"misses":{max},"unconsulted":{max}}}"#,
        max = u64::MAX
    );
    assert_eq!(
        parse(huge.as_bytes()),
        Ok(Some(session(u64::MAX, u64::MAX, u64::MAX)))
    );
    assert_eq!(parse(report_json(5, 0, 0, 0).as_bytes()), Ok(None));
    assert!(parse(br#"{"version":5,"hits":1}"#).is_err_and(|e| e.contains("without its counts")));
    assert!(parse(br#"{"hits":1}"#).is_err_and(|e| e.contains("no numeric version")));
    assert!(parse(b"{").is_err_and(|e| e.contains("not JSON")));
}
