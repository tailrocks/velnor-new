use crate::error::StateError;
use crate::identity::{AcquireIntentId, Epoch, GrantId, ProvisionIntentId, RequestId};
use crate::ownership::{CleanupProof, OwnedIds, OwnershipFailure};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerState {
    Reserved {
        grant: GrantId,

        epoch: Epoch,
    },

    Acquiring {
        intent: AcquireIntentId,

        epoch: Epoch,
    },

    AcquisitionUncertain {
        intent: AcquireIntentId,

        epoch: Epoch,
    },

    Provisioning {
        intent: ProvisionIntentId,

        epoch: Epoch,
    },

    IdleAssignable {
        epoch: Epoch,
    },

    Running {
        epoch: Epoch,
    },

    Finishing {
        epoch: Epoch,
    },

    Cleaning {
        epoch: Epoch,

        owned: OwnedIds,
    },

    Quarantined {
        epoch: Epoch,

        reason: OwnershipFailure,
    },

    Released,
}

impl WorkerState {
    #[must_use]
    pub const fn epoch(&self) -> Option<Epoch> {
        match self {
            Self::Reserved { epoch, .. }
            | Self::Acquiring { epoch, .. }
            | Self::AcquisitionUncertain { epoch, .. }
            | Self::Provisioning { epoch, .. }
            | Self::IdleAssignable { epoch }
            | Self::Running { epoch }
            | Self::Finishing { epoch }
            | Self::Cleaning { epoch, .. }
            | Self::Quarantined { epoch, .. } => Some(*epoch),
            Self::Released => None,
        }
    }

    #[must_use]
    pub const fn counts_capacity(&self) -> bool {
        !matches!(self, Self::Released)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Advance,

    Idempotent,

    ReleaseCapacity,

    KeepCapacity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub next: WorkerState,

    pub effect: Effect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerEvent {
    AcquireStarted {
        intent: AcquireIntentId,

        epoch: Epoch,
    },

    Acquired {
        epoch: Epoch,
    },

    AcquireUncertain {
        epoch: Epoch,
    },

    ProvisionStarted {
        intent: ProvisionIntentId,

        epoch: Epoch,
    },

    Idle {
        epoch: Epoch,
    },

    Running {
        epoch: Epoch,
    },

    Finishing {
        epoch: Epoch,
    },

    Cleaning {
        epoch: Epoch,

        owned: OwnedIds,
    },

    CleanupVerified {
        epoch: Epoch,

        proof: CleanupProof,
    },

    OwnershipMismatch {
        epoch: Epoch,

        reason: OwnershipFailure,
    },

    Redelivered {
        id: RequestId,
    },
}

pub fn transition(state: &WorkerState, event: &WorkerEvent) -> Result<Transition, StateError> {
    if let WorkerEvent::Redelivered { .. } = event {
        return Ok(Transition {
            next: state.clone(),
            effect: Effect::Idempotent,
        });
    }
    if let WorkerEvent::OwnershipMismatch { epoch, reason } = event {
        return quarantine(state, *epoch, *reason);
    }
    require_same(state, event_epoch(event))?;
    step(state, event)
}

fn quarantine(
    state: &WorkerState,
    epoch: Epoch,
    reason: OwnershipFailure,
) -> Result<Transition, StateError> {
    require_same(state, epoch)?;
    if matches!(state, WorkerState::Released) {
        return Err(StateError::IllegalTransition);
    }
    Ok(Transition {
        next: WorkerState::Quarantined { epoch, reason },
        effect: Effect::KeepCapacity,
    })
}

fn require_same(state: &WorkerState, epoch: Epoch) -> Result<(), StateError> {
    match state.epoch() {
        Some(current) if current == epoch => Ok(()),
        Some(_) => Err(StateError::StaleEpoch),
        None => Err(StateError::IllegalTransition),
    }
}

fn event_epoch(event: &WorkerEvent) -> Epoch {
    match event {
        WorkerEvent::AcquireStarted { epoch, .. }
        | WorkerEvent::Acquired { epoch }
        | WorkerEvent::AcquireUncertain { epoch }
        | WorkerEvent::ProvisionStarted { epoch, .. }
        | WorkerEvent::Idle { epoch }
        | WorkerEvent::Running { epoch }
        | WorkerEvent::Finishing { epoch }
        | WorkerEvent::Cleaning { epoch, .. }
        | WorkerEvent::CleanupVerified { epoch, .. }
        | WorkerEvent::OwnershipMismatch { epoch, .. } => *epoch,
        WorkerEvent::Redelivered { .. } => Epoch::new(0),
    }
}

fn step(state: &WorkerState, event: &WorkerEvent) -> Result<Transition, StateError> {
    match (state, event) {
        (WorkerState::Reserved { epoch, .. }, WorkerEvent::AcquireStarted { intent, .. }) => {
            Ok(advance(WorkerState::Acquiring {
                intent: *intent,
                epoch: *epoch,
            }))
        }
        (
            WorkerState::Acquiring { intent, .. },
            WorkerEvent::AcquireStarted { intent: again, .. },
        ) if intent == again => Ok(Transition {
            next: state.clone(),
            effect: Effect::Idempotent,
        }),
        (WorkerState::Acquiring { .. }, WorkerEvent::Acquired { .. }) => Ok(Transition {
            next: state.clone(),
            effect: Effect::Idempotent,
        }),
        (WorkerState::Acquiring { intent, epoch }, WorkerEvent::AcquireUncertain { .. }) => {
            Ok(Transition {
                next: WorkerState::AcquisitionUncertain {
                    intent: *intent,
                    epoch: *epoch,
                },
                effect: Effect::KeepCapacity,
            })
        }
        (WorkerState::Acquiring { epoch, .. }, WorkerEvent::ProvisionStarted { intent, .. }) => {
            Ok(advance(WorkerState::Provisioning {
                intent: *intent,
                epoch: *epoch,
            }))
        }
        (WorkerState::Provisioning { epoch, .. }, WorkerEvent::Idle { .. }) => {
            Ok(advance(WorkerState::IdleAssignable { epoch: *epoch }))
        }
        (WorkerState::IdleAssignable { epoch }, WorkerEvent::Running { .. }) => {
            Ok(advance(WorkerState::Running { epoch: *epoch }))
        }
        (WorkerState::Running { epoch }, WorkerEvent::Finishing { .. }) => {
            Ok(advance(WorkerState::Finishing { epoch: *epoch }))
        }
        (WorkerState::Finishing { epoch }, WorkerEvent::Cleaning { owned, .. }) => {
            Ok(advance(WorkerState::Cleaning {
                epoch: *epoch,
                owned: owned.clone(),
            }))
        }
        (
            WorkerState::Cleaning { owned, .. },
            WorkerEvent::CleanupVerified { proof, epoch, .. },
        ) => verified(owned, proof, *epoch),
        _ => Err(StateError::IllegalTransition),
    }
}

fn advance(next: WorkerState) -> Transition {
    Transition {
        next,
        effect: Effect::Advance,
    }
}

fn verified(
    owned: &OwnedIds,
    proof: &CleanupProof,
    _epoch: Epoch,
) -> Result<Transition, StateError> {
    if !proof.covers(owned) {
        return Err(StateError::CleanupMismatch);
    }
    Ok(Transition {
        next: WorkerState::Released,
        effect: Effect::ReleaseCapacity,
    })
}
