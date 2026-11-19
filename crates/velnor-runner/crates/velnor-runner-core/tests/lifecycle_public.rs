// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_core::{
    CleanupProof, Effect, Epoch, OwnedIds, OwnershipFailure, StateError, WorkerEvent, WorkerState,
    transition,
};

fn owned() -> OwnedIds {
    OwnedIds {
        container_id: "ab".repeat(32),
        volume: "vol-home".to_owned(),
    }
}

fn mismatch(reason: OwnershipFailure) -> WorkerEvent {
    WorkerEvent::OwnershipMismatch {
        epoch: Epoch::new(7),
        reason,
    }
}

#[test]
fn quarantine_from_cleaning_keeps_the_owned_ids_and_the_slot() {
    for reason in [
        OwnershipFailure::MissingResponse,
        OwnershipFailure::IdMismatch,
        OwnershipFailure::ForeignObject,
    ] {
        let cleaning = WorkerState::Cleaning {
            epoch: Epoch::new(7),
            owned: owned(),
        };
        let result = transition(&cleaning, &mismatch(reason)).expect("quarantine is legal");
        assert_eq!(result.effect, Effect::KeepCapacity);
        assert_eq!(result.next.owned(), Some(&owned()));
        assert!(result.next.counts_capacity());
        assert_ne!(result.next, WorkerState::Released);
    }
}

#[test]
fn quarantine_does_not_release_on_a_cleanup_proof() {
    let cleaning = WorkerState::Cleaning {
        epoch: Epoch::new(7),
        owned: owned(),
    };
    let quarantined = transition(&cleaning, &mismatch(OwnershipFailure::MissingResponse))
        .expect("quarantine is legal")
        .next;
    let proof = WorkerEvent::CleanupVerified {
        epoch: Epoch::new(7),
        proof: CleanupProof {
            container_id: owned().container_id,
            volume: owned().volume,
        },
    };
    assert_eq!(
        transition(&quarantined, &proof),
        Err(StateError::IllegalTransition)
    );
}

#[test]
fn quarantine_before_cleaning_has_no_ids_to_keep() {
    let running = WorkerState::Running {
        epoch: Epoch::new(7),
    };
    let result =
        transition(&running, &mismatch(OwnershipFailure::IdMismatch)).expect("quarantine is legal");
    assert_eq!(result.next.owned(), None);
    assert!(result.next.counts_capacity());
}
