// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_core::{
    AcquireIntentId, Effect, Epoch, ProvisionIntentId, StateError, WorkerEvent, WorkerState,
    transition,
};

#[cfg(test)]
fn intent() -> AcquireIntentId {
    AcquireIntentId::new(2).expect("non-zero intent")
}

#[cfg(test)]
fn epoch() -> Epoch {
    Epoch::new(7)
}

#[cfg(test)]
fn acquiring() -> WorkerState {
    WorkerState::Acquiring {
        intent: intent(),
        epoch: epoch(),
    }
}

#[cfg(test)]
fn acquired() -> WorkerState {
    transition(&acquiring(), &WorkerEvent::Acquired { epoch: epoch() })
        .expect("Acquired is legal while acquiring")
        .next
}

#[test]
fn acquired_leaves_the_in_flight_state_and_keeps_capacity() {
    let step = transition(&acquiring(), &WorkerEvent::Acquired { epoch: epoch() })
        .expect("Acquired is legal while acquiring");

    assert_ne!(step.next, acquiring());
    assert!(step.next.counts_capacity());
    assert_ne!(step.effect, Effect::ReleaseCapacity);
    assert_eq!(step.next.epoch(), Some(epoch()));
}

#[test]
fn acquire_started_after_success_is_illegal() {
    let again = WorkerEvent::AcquireStarted {
        intent: intent(),
        epoch: epoch(),
    };

    assert_eq!(
        transition(&acquired(), &again),
        Err(StateError::IllegalTransition)
    );
}

#[test]
fn acquire_uncertain_after_success_is_illegal() {
    assert_eq!(
        transition(
            &acquired(),
            &WorkerEvent::AcquireUncertain { epoch: epoch() }
        ),
        Err(StateError::IllegalTransition)
    );
}

#[test]
fn acquire_uncertain_before_success_keeps_capacity() {
    let step = transition(
        &acquiring(),
        &WorkerEvent::AcquireUncertain { epoch: epoch() },
    )
    .expect("uncertainty is legal while acquiring");

    assert_eq!(step.effect, Effect::KeepCapacity);
    assert!(step.next.counts_capacity());
}

#[test]
fn provisioning_follows_a_recorded_acquire() {
    let provision = WorkerEvent::ProvisionStarted {
        intent: ProvisionIntentId::new(3).expect("non-zero intent"),
        epoch: epoch(),
    };

    let step = transition(&acquired(), &provision).expect("provisioning follows acquire");

    assert!(matches!(step.next, WorkerState::Provisioning { .. }));
    assert_eq!(step.effect, Effect::Advance);
}

#[test]
fn conflict_after_success_does_not_free_the_slot() {
    let conflict = WorkerEvent::OwnershipMismatch {
        epoch: epoch(),
        reason: OwnershipFailure::ForeignObject,
    };

    let step = transition(&acquired(), &conflict).expect("a conflict quarantines the worker");

    assert_eq!(step.effect, Effect::KeepCapacity);
    assert!(step.next.counts_capacity());
}
