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
    cargo_with(repo, subcommand, report, stderr, None, true, false)
}

fn cargo_with(
    repo: &Repo,
    subcommand: &str,
    report: &str,
    stderr: &str,
    inherited: Option<&std::path::Path>,
    enabled: bool,
    inside_session: bool,
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
        .env(SWITCH, if enabled { "true" } else { "false" })
        .env("FAKE_MBX_REPORT", report)
        .env("FAKE_MBX_STDERR", stderr)
        .env_remove("MBX_STATS_REPORT")
        .env_remove("MBX_SOCKET");
    if let Some(path) = inherited {
        command.env("MBX_STATS_REPORT", path);
    }
    if inside_session {
        command.env("MBX_SOCKET", repo.root().join("cache-agent.sock"));
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
        true,
        false,
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
        &["cargo", "rustc", "--", "-Zfoo"],
        &["cargo", "b"],
        &["cargo", "c"],
        &["cargo", "d"],
        &["cargo", "+1.100.0", "build"],
        &["cargo", "--config", "k=v", "build"],
        &["cargo", "--config=k=v", "--config", "a=b", "check"],
        &["cargo", "-C", "dir", "build"],
        &["cargo", "-Z", "unstable-options", "build"],
        &["cargo", "-Zunstable-options", "-Cdir", "build"],
        &["cargo", "--color", "always", "build"],
        &["cargo", "--color=never", "build"],
        &["cargo", "-v", "-vv", "-q", "--quiet", "--verbose", "build"],
        &["cargo", "--frozen", "--locked", "--offline", "check"],
        &["cargo", "-qv", "build"],
        &["cargo", "-vq", "build"],
        &["cargo", "-vC", "dir", "build"],
        &["cargo", "-vCdir", "build"],
        &["cargo", "-qZ", "unstable-options", "check"],
        &[
            "cargo", "+nightly", "-Z", "x", "-C", "d", "--locked", "clippy",
        ],
    ] {
        assert!(reports_session(&argv(build)), "{build:?}");
    }
    for runs in [
        &["cargo", "test"][..],
        &["cargo", "t"],
        &["cargo", "r"],
        &["cargo", "nextest", "run"],
        &["cargo-nextest", "nextest", "run"],
        &["cargo", "run"],
        &["cargo", "bench"],
        &["cargo", "-C", "build", "test"],
        &["cargo", "--config", "build", "test"],
        &["cargo", "--config", "k=v", "test"],
        &["cargo", "-Z", "build", "run"],
        &["cargo", "--color", "build", "test"],
        &["cargo", "+build", "test"],
        &["cargo", "--unknown", "build"],
        &["cargo", "--manifest-path", "build", "test"],
        &["cargo", "-vC", "build", "test"],
        &["cargo", "-qZ", "build", "run"],
        &["cargo", "-vh", "build"],
        &["cargo", "-C"],
        &["cargo", "--config"],
        &["cargo", "--"],
        &["cargo", "--version"],
        &["cargo", "+nightly"],
        &["cargo"],
        &["mise", "exec", "--", "cargo", "build"],
        &["env", "X=1", "cargo", "build"],
        &["velnor-ci", "inputs", "build"],
        &[],
    ] {
        assert!(!reports_session(&argv(runs)), "{runs:?}");
    }
}

#[test]
fn a_build_given_a_report_path_that_left_none_is_an_error_even_alone() -> ResultTest {
    let repo = fixture("mbx-reports-alone")?;
    cargo(&repo, "build", "", "reworded summary")?;
    let metrics = report(&repo)?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
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
fn a_build_with_a_reworded_summary_and_no_report_is_an_error_beside_another_report() -> ResultTest {
    let repo = fixture("mbx-reports-reworded")?;
    cargo(&repo, "build", &report_json(5, 1, 0, 0), "compiled")?;
    cargo(&repo, "check", "", "something new about the cache")?;
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
fn a_report_of_an_unknown_version_is_one_unknown_session_not_two() -> ResultTest {
    let repo = fixture("mbx-reports-unknown-once")?;
    cargo(
        &repo,
        "build",
        &report_json(6, 4, 0, 0),
        "mbx[cache]: object cache: 4 hits, 0 misses; 0 B downloaded",
    )?;
    let metrics = report(&repo)?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1);
    assert_eq!(metrics.verification_errors.len(), 1, "{metrics:?}");
    Ok(())
}

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
        plant_session(&repo, &format!("{index:05}"), Some(&report_json(5, 1, 0, 0)))?;
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
        plant_session(&repo, &format!("{index:05}"), Some(&report_json(5, 1, 0, 0)))?;
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
        plant_session(&repo, &format!("{index:05}"), Some(&report_json(5, 1, 0, 0)))?;
    }
    let metrics = report(&repo)?;
    assert!(metrics.mbx.sessions.is_empty(), "{}", metrics.mbx.sessions.len());
    assert_eq!(metrics.mbx.unparsed, 1);
    assert!(metrics.verification_errors.is_empty(), "{:?}", metrics.verification_errors);
    assert!(velnor_ci::report::check(std::slice::from_ref(&metrics)).is_ok());
    Ok(())
}

#[test]
fn a_dangling_json_symlink_is_one_unreadable_report_not_also_missing() -> ResultTest {
    let repo = fixture("mbx-reports-dangling")?;
    cargo(&repo, "build", &report_json(5, 1, 0, 0), "compiled")?;
    plant_session(&repo, "5-dangling", None)?;
    std::os::unix::fs::symlink(repo.root().join("nowhere.json"), plant(&repo, "5-dangling.json")?)?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(1, 0, 0)], "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    let errors = metrics.verification_errors.join("\n");
    assert!(errors.contains("(1 report(s)); first: 5-dangling.json: not a regular file"), "{errors}");
    assert!(!errors.contains("missing"), "{errors}");
    Ok(())
}

#[test]
fn a_session_inside_an_enclosing_mbx_session_gets_no_report_path() -> ResultTest {
    let repo = fixture("mbx-reports-nested")?;
    cargo_with(&repo, "build", &report_json(5, 1, 0, 0), "compiled", None, true, true)?;
    assert!(!repo.root().join("velnor-mbx-reports").exists());
    let metrics = report(&repo)?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}

#[test]
fn with_the_switch_off_an_inherited_report_path_never_reaches_the_child() -> ResultTest {
    let repo = fixture("mbx-reports-off")?;
    let inherited = repo.root().join("outer-report.json");
    cargo_with(
        &repo,
        "build",
        &report_json(5, 1, 0, 0),
        "compiled",
        Some(&inherited),
        false,
        false,
    )?;
    assert!(!inherited.exists());
    Ok(())
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
