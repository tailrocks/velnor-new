// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_core::{
    AcquireIntentId, Capacity, CleanupProof, Effect, Epoch, OwnedIds, StateError, WorkerEvent,
    WorkerId, WorkerState, transition,
};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

const FULL_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const UPPER_ID: &str = "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
const NON_HEX_ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdeg";
const SHORT_ID: &str = "0123456789ab";
const WORKER_NAME: &str = "w0123456789abcdef0123456789abcdef-runner";

fn ids(container_id: &str, volume: &str) -> (OwnedIds, CleanupProof) {
    (
        OwnedIds {
            container_id: container_id.to_owned(),
            volume: volume.to_owned(),
        },
        CleanupProof {
            container_id: container_id.to_owned(),
            volume: volume.to_owned(),
        },
    )
}

fn cleaning_capacity(owned: &OwnedIds) -> TestResult<(Capacity, WorkerId)> {
    let worker = WorkerId::new(1)?;
    let mut capacity = Capacity::new(1);
    capacity.reserve(worker, AcquireIntentId::new(1)?, Epoch::new(1), 0, 0)?;
    capacity.store(
        worker,
        WorkerState::Cleaning {
            epoch: Epoch::new(1),
            owned: owned.clone(),
        },
    )?;
    Ok((capacity, worker))
}

#[test]
fn only_a_full_container_id_and_a_named_volume_release_capacity() -> TestResult<()> {
    for (container_id, volume) in [
        (SHORT_ID, "vol-work"),
        (FULL_ID, ""),
        (WORKER_NAME, "vol-work"),
        (UPPER_ID, "vol-work"),
        (NON_HEX_ID, "vol-work"),
    ] {
        let (owned, proof) = ids(container_id, volume);
        let (mut capacity, worker) = cleaning_capacity(&owned)?;
        assert_eq!(
            capacity.release(worker, &proof),
            Err(StateError::CleanupMismatch),
            "{container_id:?} {volume:?}"
        );
        assert_eq!(capacity.occupancy(), 1);
        let verified = transition(
            &WorkerState::Cleaning {
                epoch: Epoch::new(1),
                owned,
            },
            &WorkerEvent::CleanupVerified {
                epoch: Epoch::new(1),
                proof,
            },
        );
        assert_eq!(verified, Err(StateError::CleanupMismatch));
    }
    Ok(())
}

#[test]
fn a_full_container_id_with_an_equal_named_volume_releases() -> TestResult<()> {
    let (owned, proof) = ids(FULL_ID, "vol-work");
    let (mut capacity, worker) = cleaning_capacity(&owned)?;
    capacity.release(worker, &proof)?;
    assert_eq!(capacity.occupancy(), 0);
    let verified = transition(
        &WorkerState::Cleaning {
            epoch: Epoch::new(1),
            owned,
        },
        &WorkerEvent::CleanupVerified {
            epoch: Epoch::new(1),
            proof,
        },
    )?;
    assert_eq!(verified.next, WorkerState::Released);
    assert_eq!(verified.effect, Effect::ReleaseCapacity);
    Ok(())
}
