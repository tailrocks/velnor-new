// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex};
use std::thread;

use velnor_runner_core::{
    AcquireIntentId, Capacity, CleanupProof, Epoch, OwnedIds, StateError, WorkerId, WorkerState,
};

fn worker(raw: u64) -> WorkerId {
    WorkerId::new(raw).expect("nonzero worker")
}

fn intent(raw: u64) -> AcquireIntentId {
    AcquireIntentId::new(raw).expect("nonzero intent")
}

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
fn concurrent_reserve_and_release_keep_the_count_exact() {
    let capacity = Arc::new(Mutex::new(Capacity::new(4)));
    for raw in 1..=4 {
        let mut guard = capacity.lock().expect("lock");
        guard
            .reserve(worker(raw), intent(raw), Epoch::new(1), raw, raw)
            .expect("seed");
        guard.store(worker(raw), cleaning()).expect("store");
    }

    let mut handles = Vec::new();
    for raw in 1..=4 {
        let capacity = Arc::clone(&capacity);
        handles.push(thread::spawn(move || {
            for round in 0..50 {
                let mut guard = capacity.lock().expect("lock");
                let _ = guard.reserve(worker(raw), intent(100 + round), Epoch::new(2), 9, 9);
            }
        }));
    }
    for raw in 1..=2 {
        let capacity = Arc::clone(&capacity);
        handles.push(thread::spawn(move || {
            capacity
                .lock()
                .expect("lock")
                .release(worker(raw), &proof())
                .expect("release original");
        }));
    }
    for handle in handles {
        handle.join().expect("thread");
    }

    let guard = capacity.lock().expect("lock");
    let released = [worker(1), worker(2)];
    let mut live = 0;
    for raw in 1..=4 {
        let id = worker(raw);
        if released.contains(&id) {
            continue;
        }
        assert_eq!(guard.state(id), Some(&cleaning()));
        live += 1;
    }
    assert!(guard.occupancy() >= live);
    assert!(guard.occupancy() <= 4);
}
