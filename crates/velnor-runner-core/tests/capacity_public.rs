// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex};
use std::thread;

use velnor_runner_core::{Capacity, CleanupProof, Epoch, OwnedIds, StateError, WorkerState};

#[cfg(test)]
mod ids {
    use velnor_runner_core::{AcquireIntentId, WorkerId};

    pub(crate) fn worker(raw: u64) -> WorkerId {
        WorkerId::new(raw).expect("nonzero worker")
    }

    pub(crate) fn intent(raw: u64) -> AcquireIntentId {
        AcquireIntentId::new(raw).expect("nonzero intent")
    }
}

use ids::{intent, worker};

fn owned() -> OwnedIds {
    OwnedIds {
        container_id: "ab".repeat(32),
        volume: "vol-home".to_owned(),
    }
}

fn proof() -> CleanupProof {
    let owned = owned();
    CleanupProof {
        container_id: owned.container_id,
        volume: owned.volume,
    }
}

fn cleaning() -> WorkerState {
    WorkerState::Cleaning {
        epoch: Epoch::new(1),
        owned: owned(),
    }
}

#[test]
fn reserve_never_overwrites_a_live_slot_with_a_new_intent() {
    let mut capacity = Capacity::new(2);
    capacity
        .reserve(worker(1), intent(1), Epoch::new(1), 1, 1)
        .expect("first reserve");
    capacity.store(worker(1), cleaning()).expect("store");

    let second = capacity.reserve(worker(1), intent(2), Epoch::new(2), 2, 2);

    assert_eq!(second, Err(StateError::DuplicateAcquire));
    assert_eq!(capacity.occupancy(), 1);
    assert_eq!(capacity.state(worker(1)), Some(&cleaning()));
    capacity
        .release(worker(1), &proof())
        .expect("original proof still releases");
    assert_eq!(capacity.occupancy(), 0);
}

#[test]
fn same_intent_replay_returns_original_grant_without_a_second_slot() {
    let mut capacity = Capacity::new(2);
    let grant = capacity
        .reserve(worker(1), intent(1), Epoch::new(1), 1, 1)
        .expect("first reserve");

    let replay = capacity.reserve(worker(1), intent(1), Epoch::new(1), 1, 1);
    let other_worker = capacity.reserve(worker(2), intent(1), Epoch::new(1), 2, 2);

    assert_eq!(replay, Ok(grant));
    assert_eq!(other_worker, Ok(grant));
    assert_eq!(capacity.occupancy(), 1);
    assert_eq!(capacity.state(worker(2)), None);
}

#[test]
fn released_worker_may_reserve_again_with_a_new_intent() {
    let mut capacity = Capacity::new(1);
    capacity
        .reserve(worker(1), intent(1), Epoch::new(1), 1, 1)
        .expect("first reserve");
    capacity.store(worker(1), cleaning()).expect("store");
    capacity.release(worker(1), &proof()).expect("release");

    let again = capacity.reserve(worker(1), intent(2), Epoch::new(2), 2, 2);

    assert!(again.is_ok());
    assert_eq!(capacity.occupancy(), 1);
}

#[test]
fn concurrent_reserves_after_releases_only_refill_the_released_slots() {
    let capacity = Arc::new(Mutex::new(Capacity::new(4)));
    for raw in 1..=4 {
        let mut guard = capacity.lock().expect("lock");
        guard
            .reserve(worker(raw), intent(raw), Epoch::new(1), raw, raw)
            .expect("seed");
        guard.store(worker(raw), cleaning()).expect("store");
    }

    let releases: Vec<_> = (1..=2)
        .map(|raw| {
            let capacity = Arc::clone(&capacity);
            thread::spawn(move || {
                capacity
                    .lock()
                    .expect("lock")
                    .release(worker(raw), &proof())
                    .expect("release original");
            })
        })
        .collect();
    for handle in releases {
        handle.join().expect("release thread");
    }

    let reserves: Vec<_> = (1..=4)
        .map(|raw| {
            let capacity = Arc::clone(&capacity);
            thread::spawn(move || {
                for round in 0..50 {
                    let unique = 1000 * raw + round;
                    let reserved = capacity.lock().expect("lock").reserve(
                        worker(raw),
                        intent(unique),
                        Epoch::new(2),
                        9,
                        9,
                    );
                    assert_eq!(reserved.is_ok(), raw <= 2 && round == 0);
                }
            })
        })
        .collect();
    for handle in reserves {
        handle.join().expect("reserve thread");
    }

    let guard = capacity.lock().expect("lock");
    for raw in 1..=2 {
        assert!(matches!(
            guard.state(worker(raw)),
            Some(WorkerState::Reserved { .. })
        ));
    }
    for raw in 3..=4 {
        assert_eq!(guard.state(worker(raw)), Some(&cleaning()));
    }
    assert_eq!(guard.occupancy(), 4);
}
