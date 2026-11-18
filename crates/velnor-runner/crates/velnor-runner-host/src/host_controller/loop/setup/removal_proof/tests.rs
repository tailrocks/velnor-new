// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::prove_and_record;
use crate::host_controller::{fixture_config, read_absence_proofs, removed_fixture};
use crate::scale_set::{EnsureError, RunnerProver};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// What GitHub said about each `(scale_set_id, runner_id)`; anything unlisted is "not our set".
struct Scripted(BTreeMap<(i64, i64), Result<bool, EnsureError>>);

impl RunnerProver for Scripted {
    fn prove_absent(&mut self, scale_set_id: i64, runner_id: i64) -> Result<bool, EnsureError> {
        self.0
            .get(&(scale_set_id, runner_id))
            .copied()
            .unwrap_or(Ok(false))
    }
}

fn targets(pairs: &[(i64, i64)]) -> BTreeSet<(i64, i64)> {
    pairs.iter().copied().collect()
}

#[test]
fn a_confirmed_deregistration_writes_the_removal_proof() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = fixture_config("conn-test-1")?.connections()[0].clone();
    removed_fixture(state.path(), &binding);
    let mut prover = Scripted(BTreeMap::from([((10, 77), Ok(true))]));
    let proven = prove_and_record(state.path(), &binding, &targets(&[(10, 77)]), &mut prover)?;
    assert_eq!(proven, 1);
    let proofs = read_absence_proofs(state.path(), &fixture_config("conn-other-1")?);
    assert!(proofs.contains(10, 77));
    Ok(())
}

#[test]
fn a_failing_deletion_writes_no_proof() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = fixture_config("conn-test-1")?.connections()[0].clone();
    removed_fixture(state.path(), &binding);
    for failure in [
        EnsureError::Uncertain,
        EnsureError::Forbidden,
        EnsureError::Unauthorized,
    ] {
        let mut prover = Scripted(BTreeMap::from([((10, 77), Err(failure))]));
        let proven = prove_and_record(state.path(), &binding, &targets(&[(10, 77)]), &mut prover)?;
        assert_eq!(proven, 0);
        let record = state
            .path()
            .join("connection-drain/conn-test-1~github-absent");
        assert!(!record.exists(), "{failure:?} left a proof record");
    }
    Ok(())
}

#[test]
fn only_the_runners_that_were_confirmed_are_recorded() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = fixture_config("conn-test-1")?.connections()[0].clone();
    removed_fixture(state.path(), &binding);
    let mut prover = Scripted(BTreeMap::from([
        ((10, 77), Ok(true)),
        ((10, 78), Err(EnsureError::Uncertain)),
        ((10, 79), Ok(false)),
    ]));
    let all = targets(&[(10, 77), (10, 78), (10, 79)]);
    assert_eq!(
        prove_and_record(state.path(), &binding, &all, &mut prover)?,
        1
    );
    let proofs = read_absence_proofs(state.path(), &fixture_config("conn-other-1")?);
    assert!(proofs.contains(10, 77));
    assert!(!proofs.contains(10, 78));
    assert!(!proofs.contains(10, 79));
    Ok(())
}

#[test]
fn a_later_run_keeps_what_an_earlier_run_proved() -> TestResult {
    let state = tempfile::tempdir()?;
    let binding = fixture_config("conn-test-1")?.connections()[0].clone();
    removed_fixture(state.path(), &binding);
    let mut first = Scripted(BTreeMap::from([((10, 77), Ok(true))]));
    prove_and_record(state.path(), &binding, &targets(&[(10, 77)]), &mut first)?;
    let mut second = Scripted(BTreeMap::from([((10, 78), Ok(true))]));
    prove_and_record(state.path(), &binding, &targets(&[(10, 78)]), &mut second)?;
    let proofs = read_absence_proofs(state.path(), &fixture_config("conn-other-1")?);
    assert!(proofs.contains(10, 77) && proofs.contains(10, 78));
    Ok(())
}
