// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mbx_reports::{cargo, cargo_with, report, report_json};
use super::{ResultTest, fixture};
use serde_json::json;
use std::fs;
use std::os::unix::net::UnixListener;

fn markers(repo: &super::Repo) -> usize {
    reports(repo)
        .iter()
        .filter(|name| name.ends_with(".expected"))
        .count()
}

fn reports(repo: &super::Repo) -> Vec<String> {
    fs::read_dir(repo.root().join("velnor-mbx-reports"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_stale_socket_path_does_not_make_a_cargo_call_look_nested() -> ResultTest {
    let repo = fixture("mbx-reports-stale-socket")?;
    let dead = repo.root().join("dead.sock");
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
fn a_socket_with_no_daemon_behind_it_is_not_a_live_session() -> ResultTest {
    let repo = fixture("mbx-reports-orphan-socket")?;
    let orphan = repo.root().join("orphan.sock");
    drop(UnixListener::bind(&orphan)?);
    cargo_with(
        &repo,
        "build",
        &report_json(5, 1, 0, 0),
        "compiled",
        None,
        true,
        Some(&orphan),
    )?;
    assert_eq!(markers(&repo), 1);
    Ok(())
}

#[test]
fn a_report_nobody_was_assigned_does_not_change_the_counts() -> ResultTest {
    let repo = fixture("mbx-reports-unassigned")?;
    cargo(&repo, "build", &report_json(5, 1, 0, 0), "compiled")?;
    let directory = repo.root().join("velnor-mbx-reports");
    fs::write(directory.join("planted.json"), report_json(5, 99, 0, 0))?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions.len(), 1, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions[0].hits, 1);
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    let errors = metrics.verification_errors.join("\n");
    assert!(
        errors.contains("planted.json: not assigned by velnor-ci"),
        "{errors}"
    );
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
            "hits": 3, "misses": 1, "not_looked_up": 2, "verifications": 4,
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
fn a_session_without_extra_categories_keeps_its_three_counts_only() -> ResultTest {
    let repo = fixture("mbx-reports-plain-session")?;
    cargo(&repo, "build", &report_json(5, 3, 1, 0), "compiled")?;
    let metrics = report(&repo)?;
    assert_eq!(
        serde_json::to_value(&metrics.mbx)?["sessions"][0],
        json!({"hits": 3, "misses": 1, "not_looked_up": 0})
    );
    Ok(())
}
