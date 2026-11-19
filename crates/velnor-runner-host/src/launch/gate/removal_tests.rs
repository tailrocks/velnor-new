// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A removed connection has no binding in the current config. Its legacy rows release only on the
//! removal-proof record of its removal flow, and hold with "connection removed" without one.

use std::collections::BTreeSet;
use std::path::Path;

use zeroize::Zeroizing;

use super::{GithubSources, decide, hold, observe_with};
use crate::config::{HostConfig, RepositoryConnection};
use crate::host_controller::{
    fixture_config, read_absence_proofs, record_absence, removed_fixture,
};
use crate::journal::IntentState;
use crate::reconcile::{IntentRow, Reconcile};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// The connection the operator removed.
fn binding() -> TestResult<RepositoryConnection> {
    Ok(fixture_config("conn-test-1")?.connections()[0].clone())
}

/// The config after the removal: another connection remains, with a usable credential, but
/// GitHub says it does not own the removed connection's scale set.
fn config_after_removal() -> TestResult<HostConfig> {
    fixture_config("conn-other-1")
}

const SOURCES: GithubSources = GithubSources {
    credential: |_| Ok(Zeroizing::new(b"token".to_vec())),
    list: |_, _, _| Ok(None),
};

fn legacy_row(id: i64, runner: &str) -> IntentRow {
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
        cleanup_proven: false,
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

fn gate_decision(rows: &[IntentRow], state: &Path) -> TestResult<(Reconcile, Vec<String>)> {
    let config = config_after_removal()?;
    let proofs = read_absence_proofs(state, &config);
    let observed = observe_with(rows, &config, &proofs, &SOURCES);
    let lines = hold::hold_lines(rows, &[], &[], &observed);
    Ok((decide(rows, &[], observed.runners), lines))
}

fn record(state: &Path, binding: &RepositoryConnection, pairs: &[(i64, i64)]) -> TestResult {
    record_absence(
        state,
        binding,
        &pairs.iter().copied().collect::<BTreeSet<_>>(),
    )?;
    Ok(())
}

#[test]
fn a_removed_connection_with_its_removal_proof_releases_admission() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = binding()?;
    removed_fixture(state.path(), &binding);
    record(state.path(), &binding, &[(10, 77), (10, 78)])?;
    let rows = [legacy_row(1, "77"), legacy_row(2, "78")];
    let (decision, lines) = gate_decision(&rows, state.path())?;
    assert!(matches!(decision, Reconcile::Advertise { .. }), "{lines:?}");
    assert_eq!(lines, Vec::<String>::new());
    Ok(())
}

#[test]
fn a_removed_connection_without_the_proof_still_holds_as_connection_removed() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = binding()?;
    removed_fixture(state.path(), &binding);
    let rows = [legacy_row(1, "77")];
    let (decision, lines) = gate_decision(&rows, state.path())?;
    assert!(matches!(decision, Reconcile::Hold { occupied: 1, .. }));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("why=\"connection removed\""), "{lines:?}");
    Ok(())
}

#[test]
fn a_proof_that_covers_only_some_runners_still_holds_the_rest() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = binding()?;
    removed_fixture(state.path(), &binding);
    record(state.path(), &binding, &[(10, 77)])?;
    let rows = [legacy_row(1, "77"), legacy_row(2, "78")];
    let (decision, lines) = gate_decision(&rows, state.path())?;
    assert!(matches!(decision, Reconcile::Hold { .. }));
    assert!(lines.iter().all(|line| line.contains("connection removed")));
    Ok(())
}

#[test]
fn a_proof_for_another_scale_set_or_runner_does_not_release() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = binding()?;
    removed_fixture(state.path(), &binding);
    record(state.path(), &binding, &[(11, 77), (10, 770)])?;
    let (decision, _) = gate_decision(&[legacy_row(1, "77")], state.path())?;
    assert!(matches!(decision, Reconcile::Hold { .. }));
    Ok(())
}

#[test]
fn a_stale_proof_from_an_earlier_removal_does_not_release() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = binding()?;
    removed_fixture(state.path(), &binding);
    record(state.path(), &binding, &[(10, 77)])?;
    // The connection is removed again: a new marker token supersedes the recorded proof.
    let marker = state.path().join("connection-drain").join("conn-test-1");
    let new_token = "fedcba9876543210fedcba9876543210";
    std::fs::write(&marker, new_token)?;
    let (decision, _) = gate_decision(&[legacy_row(1, "77")], state.path())?;
    assert!(matches!(decision, Reconcile::Hold { .. }));
    Ok(())
}

#[test]
fn a_proof_is_ignored_while_the_connection_is_back_in_the_config() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = binding()?;
    removed_fixture(state.path(), &binding);
    record(state.path(), &binding, &[(10, 77)])?;
    let config = fixture_config("conn-test-1")?;
    assert!(!read_absence_proofs(state.path(), &config).contains(10, 77));
    Ok(())
}

#[test]
fn a_record_without_a_current_drain_proof_is_not_written() -> TestResult {
    let state = tempfile::tempdir()?;
    record(state.path(), &binding()?, &[(10, 77)])?;
    let config = config_after_removal()?;
    assert!(!read_absence_proofs(state.path(), &config).contains(10, 77));
    Ok(())
}
