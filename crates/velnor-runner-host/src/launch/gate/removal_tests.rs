// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A removed connection has no binding in the current config. Its legacy rows release only once the
//! removal flow settled them, and hold with "connection removed" until then.

use zeroize::Zeroizing;

use super::{GithubSources, decide, hold, observe_with};
use crate::config::HostConfig;
use crate::host_controller::fixture_config;
use crate::journal::IntentState;
use crate::reconcile::{IntentRow, Reconcile};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// The config after the removal: another connection remains, with a usable credential, but
/// GitHub says it does not own the removed connection's scale set.
fn config_after_removal() -> TestResult<HostConfig> {
    fixture_config("conn-other-1")
}

const SOURCES: GithubSources = GithubSources {
    credential: |_| Ok(Zeroizing::new(b"token".to_vec())),
    list: |_, _, _| Ok(None),
};

fn legacy_row(id: i64, runner: &str, cleanup_proven: bool) -> IntentRow {
    IntentRow {
        id,
        kind: "launch".to_owned(),
        subject: "subject".to_owned(),
        state: IntentState::Done,
        docker_id: None,
        dind_id: None,
        worker_volume: None,
        scale_set_id: Some(10),
        request_id: None,
        runner_name: None,
        docker_engine_id: None,
        launch_phase: None,
        github_runner_id: Some(runner.to_owned()),
        cleanup_proven,
        launch_id: None,
        assignment_key: None,
        seed_generation_id: None,
        acquire_attempted: false,
        acquire_resolved: false,
        acquired: false,
        jit_requested: false,
        runner_completed: false,
    }
}

fn gate_decision(rows: &[IntentRow]) -> TestResult<(Reconcile, Vec<String>)> {
    let config = config_after_removal()?;
    let observed = observe_with(rows, &config, &SOURCES);
    let lines = hold::hold_lines(rows, &[], &[], &observed);
    Ok((decide(rows, &[], observed.runners), lines))
}

#[test]
fn a_removed_connection_whose_rows_were_settled_releases_admission() -> TestResult {
    let rows = [legacy_row(1, "77", true), legacy_row(2, "78", true)];
    let (decision, lines) = gate_decision(&rows)?;
    assert!(matches!(decision, Reconcile::Advertise { .. }), "{lines:?}");
    assert_eq!(lines, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_removed_connection_with_unsettled_rows_still_holds_as_connection_removed() -> TestResult {
    let rows = [legacy_row(1, "77", false)];
    let (decision, lines) = gate_decision(&rows)?;
    assert!(matches!(decision, Reconcile::Hold { occupied: 1, .. }));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("why=\"connection removed\""), "{lines:?}");
    Ok(())
}

#[test]
fn settling_only_some_rows_still_holds_the_rest() -> TestResult {
    let rows = [legacy_row(1, "77", true), legacy_row(2, "78", false)];
    let (decision, lines) = gate_decision(&rows)?;
    assert!(matches!(decision, Reconcile::Hold { .. }));
    assert!(lines.iter().all(|line| line.contains("connection removed")));
    Ok(())
}
