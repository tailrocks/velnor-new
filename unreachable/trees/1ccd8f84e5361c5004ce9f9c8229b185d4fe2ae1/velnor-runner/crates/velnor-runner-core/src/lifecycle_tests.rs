//! Occupied states do not release on failure, quarantine, or an illegal event.

use crate::{
    AcquireIntentId, CleanupProof, Effect, Epoch, GrantId, IdError, OwnedIds, OwnershipFailure,
    ProvisionIntentId, StateError, Transition, WorkerEvent, WorkerState, transition,
};

fn current_epoch() -> Epoch {
    Epoch::new(7)
}

fn owned_ids() -> OwnedIds {
    OwnedIds {
        container_id: "c1".to_owned(),
        volume: "v1".to_owned(),
    }
}

fn occupied() -> Result<Vec<WorkerState>, IdError> {
    let epoch = current_epoch();
    Ok(vec![
        WorkerState::Reserved {
            grant: GrantId::new(1)?,
            epoch,
        },
        WorkerState::Acquiring {
            intent: AcquireIntentId::new(2)?,
            epoch,
        },
        WorkerState::AcquisitionUncertain {
            intent: AcquireIntentId::new(2)?,
            epoch,
        },
        WorkerState::Provisioning {
            intent: ProvisionIntentId::new(3)?,
            epoch,
        },
        WorkerState::IdleAssignable { epoch },
        WorkerState::Running { epoch },
        WorkerState::Finishing { epoch },
        WorkerState::Cleaning {
            epoch,
            owned: owned_ids(),
        },
        WorkerState::Quarantined {
            epoch,
            reason: OwnershipFailure::IdMismatch,
        },
    ])
}

fn quarantine_event() -> WorkerEvent {
    WorkerEvent::OwnershipMismatch {
        epoch: current_epoch(),
        reason: OwnershipFailure::IdMismatch,
    }
}

/// `Running` is legal only from idle. `Idle` is legal only from provisioning.
fn illegal_event(state: &WorkerState) -> WorkerEvent {
    let epoch = current_epoch();
    if matches!(state, WorkerState::IdleAssignable { .. }) {
        WorkerEvent::Idle { epoch }
    } else {
        WorkerEvent::Running { epoch }
    }
}

fn assert_not_released(got: &Result<Transition, StateError>) {
    let Ok(step) = got else {
        assert!(got.is_ok());
        return;
    };
    assert_ne!(step.next, WorkerState::Released);
    assert_ne!(step.effect, Effect::ReleaseCapacity);
}

fn assert_quarantine(got: &Result<Transition, StateError>) {
    assert_not_released(got);
    assert!(matches!(
        got,
        Ok(Transition {
            effect: Effect::KeepCapacity,
            next: WorkerState::Quarantined { .. },
        })
    ));
}

#[test]
fn quarantine_does_not_release_occupied_states() -> Result<(), IdError> {
    let event = quarantine_event();
    for state in occupied()? {
        assert_quarantine(&transition(&state, &event));
    }
    Ok(())
}

#[test]
fn illegal_event_does_not_release_occupied_states() -> Result<(), IdError> {
    for state in occupied()? {
        let event = illegal_event(&state);
        assert_eq!(
            transition(&state, &event),
            Err(StateError::IllegalTransition)
        );
    }
    Ok(())
}

#[test]
fn acquire_failure_and_cleanup_mismatch_do_not_release() -> Result<(), IdError> {
    let epoch = current_epoch();
    let acquiring = WorkerState::Acquiring {
        intent: AcquireIntentId::new(2)?,
        epoch,
    };
    let uncertain = transition(&acquiring, &WorkerEvent::AcquireUncertain { epoch });
    assert_not_released(&uncertain);
    assert!(matches!(
        uncertain,
        Ok(Transition {
            effect: Effect::KeepCapacity,
            next: WorkerState::AcquisitionUncertain { .. },
        })
    ));
    let cleaning = WorkerState::Cleaning {
        epoch,
        owned: owned_ids(),
    };
    let proof = CleanupProof {
        container_id: "other".to_owned(),
        volume: "v1".to_owned(),
    };
    assert_eq!(
        transition(&cleaning, &WorkerEvent::CleanupVerified { epoch, proof }),
        Err(StateError::CleanupMismatch)
    );
    Ok(())
}
