// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mbx_reports::{SWITCH, cargo, cargo_command, cargo_with, report, report_json, session};
use super::mbx_socket::SocketDir;
use super::{ResultTest, fixture};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use velnor_ci::mbx_report::parse;
use velnor_ci::waste_evidence::{MbxCompilerWork, MbxSession};

fn markers(repo: &super::Repo) -> usize {
    reports(repo)
        .iter()
        .filter(|name| name.ends_with(".expected"))
        .count()
}

fn reports(repo: &super::Repo) -> Vec<String> {
    fs::read_dir(repo.root().join("velnor-mbx-reports")).map_or(Vec::new(), |entries| {
        entries
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    })
}

#[test]
fn a_stale_socket_path_does_not_make_a_cargo_call_look_nested() -> ResultTest {
    let repo = fixture("mbx-reports-stale-socket")?;
    let sockets = SocketDir::for_repo(&repo)?;
    let dead = sockets.socket("dead.sock");
    cargo_with(
        &repo,
        "build",
        &report_json(5, 2, 0, 0),
        "compiled",
        None,
        true,
        Some(&dead),
    )?;
    assert_eq!(markers(&repo), 1);
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions.len(), 1, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions[0].hits, 2);
    Ok(())
}

#[test]
fn a_socket_path_that_is_a_regular_file_is_not_a_live_session() -> ResultTest {
    let repo = fixture("mbx-reports-socket-file")?;
    let file = repo.root().join("leftover.sock");
    fs::write(&file, b"")?;
    cargo_with(
        &repo,
        "build",
        &report_json(5, 1, 0, 0),
        "compiled",
        None,
        true,
        Some(&file),
    )?;
    assert_eq!(markers(&repo), 1);
    Ok(())
}

#[test]
fn an_existing_socket_file_is_a_live_session_without_a_connection() -> ResultTest {
    let repo = fixture("mbx-reports-orphan-socket")?;
    let sockets = SocketDir::for_repo(&repo)?;
    let orphan = sockets.socket("orphan.sock");
    let _listener = UnixListener::bind(&orphan)?;
    cargo_with(
        &repo,
        "build",
        &report_json(5, 1, 0, 0),
        "compiled",
        None,
        true,
        Some(&orphan),
    )?;
    assert_eq!(markers(&repo), 0);
    Ok(())
}

#[test]
fn a_relative_socket_path_is_not_a_live_session() -> ResultTest {
    let repo = fixture("mbx-reports-relative-socket")?;
    let sockets = SocketDir::for_repo(&repo)?;
    let _listener = UnixListener::bind(sockets.socket("cache-agent.sock"))?;
    cargo_with(
        &repo,
        "build",
        &report_json(5, 2, 0, 0),
        "compiled",
        None,
        true,
        Some(std::path::Path::new("cache-agent.sock")),
    )?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(2, 0, 0)], "{metrics:?}");
    Ok(())
}

#[test]
fn a_socket_whose_listener_never_accepts_does_not_block_the_check() -> ResultTest {
    let repo = fixture("mbx-reports-stuck-listener")?;
    let sockets = SocketDir::for_repo(&repo)?;
    let stuck = sockets.socket("stuck.sock");
    let _listener = UnixListener::bind(&stuck)?;
    let mut command = cargo_command(
        &repo,
        "build",
        &report_json(5, 2, 0, 0),
        "compiled",
        None,
        true,
        Some(&stuck),
    )?;
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || sender.send(child.wait_with_output()));
    let Ok(output) = receiver.recv_timeout(Duration::from_secs(5)) else {
        Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()?;
        return Err("velnor-ci blocked on a socket whose listener never accepts".into());
    };
    let output = output?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(markers(&repo), 0);
    Ok(())
}

#[test]
fn every_mbx_outcome_category_of_a_report_reaches_the_metrics() -> ResultTest {
    let repo = fixture("mbx-reports-categories")?;
    let body = r#"{"version":5,"hits":3,"misses":1,"unconsulted":2,"verifications":4,"bypasses":{"compiler-query":2,"unsupported-flag":5},"compiler":{"miss":{"invocations":1,"duration_ns":900},"unconsulted":{"invocations":2,"duration_ns":268720292},"verification":{"invocations":4,"duration_ns":70}}}"#;
    cargo(&repo, "build", body, "compiled")?;
    let metrics = report(&repo)?;
    let session = serde_json::to_value(&metrics.mbx)?["sessions"][0].clone();
    assert_eq!(
        session,
        json!({
            "hits": 3, "misses": 1, "not_looked_up": 2, "verifications": 4, "bypassed": 5,
            "bypasses": {"compiler-query": 2, "unsupported-flag": 5},
            "compiler": {
                "miss": {"invocations": 1, "duration_ns": 900},
                "unconsulted": {"invocations": 2, "duration_ns": 268_720_292},
                "verification": {"invocations": 4, "duration_ns": 70}
            }
        })
    );
    Ok(())
}

#[test]
fn a_session_with_only_routine_bypasses_reaches_the_metrics() -> ResultTest {
    let repo = fixture("mbx-reports-routine-only")?;
    let body = r#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":2,"standard-input":1}}"#;
    cargo(&repo, "build", body, "compiled")?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions.len(), 1, "{metrics:?}");
    assert_eq!(
        metrics.mbx.sessions[0].bypasses,
        BTreeMap::from([
            ("compiler-query".to_owned(), 2),
            ("standard-input".to_owned(), 1)
        ])
    );
    assert!(metrics.mbx.sessions[0].probe_only, "{metrics:?}");
    Ok(())
}

#[test]
fn a_session_without_extra_categories_keeps_its_metrics_bytes() -> ResultTest {
    let repo = fixture("mbx-reports-plain-session")?;
    cargo(&repo, "build", &report_json(5, 3, 1, 0), "compiled")?;
    let metrics = report(&repo)?;
    let bytes = serde_json::to_vec_pretty(&metrics.mbx)?;
    assert_eq!(
        bytes,
        include_bytes!("../data/mbx_report/plain-session-mbx-evidence.json")
    );
    Ok(())
}

#[test]
fn a_newer_report_with_unknown_fields_and_the_older_bypasses_shape_is_still_read() {
    let newer = br#"{"version":5,"hits":1,"misses":0,"unconsulted":0,"added_later":{"a":1},"bypasses":{"compiler-query":2,"other":1},"compiler":{"miss":{"invocations":1,"duration_ns":9,"added_later":true},"future":"text"}}"#;
    assert_eq!(
        parse(newer),
        Ok(Some(MbxSession {
            bypasses: BTreeMap::from([("compiler-query".to_owned(), 2), ("other".to_owned(), 1)]),
            bypassed: 1,
            compiler: BTreeMap::from([(
                "miss".to_owned(),
                MbxCompilerWork {
                    invocations: 1,
                    duration_ns: 9
                }
            )]),
            ..session(1, 0, 0)
        }))
    );
    let non_numeric = br#"{"version":5,"hits":1,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":"2","other":1}}"#;
    assert!(
        parse(non_numeric).is_err(),
        "a string bypass count must not be zero"
    );
    let object = br#"{"version":5,"hits":1,"misses":0,"unconsulted":0,"bypasses":{"unsupported-flag":{"count":5}}}"#;
    assert!(
        parse(object).is_err(),
        "an object bypass count must not be zero"
    );
}

#[test]
fn the_reader_maps_a_real_mbx_report_and_names_what_it_cannot_read() {
    let real = include_bytes!("../data/mbx_report/mbx-1.30.0.json");
    assert_eq!(
        parse(real),
        Ok(Some(MbxSession {
            bypasses: BTreeMap::from([
                ("compiler-query".to_owned(), 2),
                ("standard-input".to_owned(), 1)
            ]),
            compiler: BTreeMap::from([(
                "unconsulted".to_owned(),
                MbxCompilerWork {
                    invocations: 2,
                    duration_ns: 268_720_292
                }
            )]),
            ..session(0, 0, 2)
        }))
    );
    let probes = br#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":2,"standard-input":1}}"#;
    assert_eq!(
        parse(probes),
        Ok(Some(MbxSession {
            bypasses: BTreeMap::from([
                ("compiler-query".to_owned(), 2),
                ("standard-input".to_owned(), 1)
            ]),
            probe_only: true,
            ..session(0, 0, 0)
        }))
    );
    let bypassed = br#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":1,"other":1}}"#;
    assert_eq!(
        parse(bypassed),
        Ok(Some(MbxSession {
            bypasses: BTreeMap::from([("compiler-query".to_owned(), 1), ("other".to_owned(), 1)]),
            bypassed: 1,
            ..session(0, 0, 0)
        }))
    );
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

#[test]
fn a_version_five_report_with_only_incremental_compilations_yields_a_session() {
    let incremental =
        br#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"incremental_compilations":15}"#;
    assert_eq!(parse(incremental), Ok(Some(session(0, 0, 15))));
}

#[test]
fn a_verification_only_report_is_a_session() {
    let verified = br#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"verifications":4,"bypasses":{},"compiler":{}}"#;
    assert_eq!(
        parse(verified),
        Ok(Some(MbxSession {
            verifications: Some(4),
            ..session(0, 0, 0)
        }))
    );
}

const WORKSPACE_CARGO: &str = "#!/bin/sh\nif [ \"$1\" = metadata ]; then printf '{\"packages\":[],\"workspace_root\":\"%s\",\"target_directory\":\"%s/target\"}' \"$PWD\" \"$PWD\"; exit 0; fi\nif [ -n \"$MBX_STATS_REPORT\" ] && [ -n \"$FAKE_MBX_REPORT\" ]; then printf '%s' \"$FAKE_MBX_REPORT\" > \"$MBX_STATS_REPORT\"; fi\n";
const TEST_STARTING_VELNOR_CI: &str =
    "#!/bin/sh\nexec \"$VELNOR_CI\" inputs build -- cargo build\n";

fn executable(path: &std::path::Path, script: &str) -> ResultTest {
    fs::create_dir_all(path.parent().ok_or("parent")?)?;
    fs::write(path, script)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn job_step(repo: &super::Repo, command: &[&str], report: &str) -> ResultTest {
    let path = format!(
        "{}:{}",
        repo.root().join("bin").display(),
        std::env::var("PATH")?
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--"])
        .args(command)
        .current_dir(repo.root())
        .env("PATH", path)
        .env("RUNNER_TEMP", repo.root())
        .env(SWITCH, "true")
        .env("VELNOR_CI", env!("CARGO_BIN_EXE_velnor-ci"))
        .env("FAKE_MBX_REPORT", report)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("MBX_STATS_REPORT")
        .env_remove("MBX_SOCKET")
        .output()?;
    assert!(output.status.success(), "{command:?}: {output:?}");
    Ok(())
}

#[test]
fn a_velnor_ci_that_the_projects_tests_start_adds_no_session_to_the_job() -> ResultTest {
    let repo = fixture("mbx-reports-test-processes")?;
    let runner = repo.root().join("test-runner");
    executable(&repo.root().join("bin/cargo"), WORKSPACE_CARGO)?;
    executable(&runner.join("cargo-nextest"), TEST_STARTING_VELNOR_CI)?;
    executable(&runner.join("cargo"), TEST_STARTING_VELNOR_CI)?;
    let wrapper = [env!("CARGO_BIN_EXE_velnor-ci"), "inputs", "build", "--"];
    let nextest = runner.join("cargo-nextest");
    let doctests = runner.join("cargo");
    job_step(
        &repo,
        &[&wrapper[..], &["cargo", "build"]].concat(),
        &report_json(5, 3, 0, 0),
    )?;
    let nextest = [nextest.to_str().ok_or("path")?, "nextest", "run"];
    job_step(&repo, &[&wrapper[..], &nextest].concat(), "")?;
    job_step(
        &repo,
        &[doctests.to_str().ok_or("path")?, "test", "--doc"],
        "",
    )?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions, [session(3, 0, 0)], "{metrics:?}");
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}
