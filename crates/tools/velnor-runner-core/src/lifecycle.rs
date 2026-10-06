//! Transitions return effects. They do not touch the network or Docker.

use crate::error::StateError;
use crate::identity::{AcquireIntentId, Epoch, GrantId, ProvisionIntentId, RequestId};
use crate::ownership::{CleanupProof, OwnedIds, OwnershipFailure};

/// One worker lifecycle. Every variant except [`WorkerState::Released`] occupies capacity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerState {
    /// Slot reserved before acquire.
    Reserved {
        /// Capacity grant.
        grant: GrantId,
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
    },
    /// Acquire intent is durable and the call is in flight.
    Acquiring {
        /// Acquire intent.
        intent: AcquireIntentId,
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
    },
    /// Transport failed. The slot stays occupied.
    AcquisitionUncertain {
        /// Acquire intent that may have landed.
        intent: AcquireIntentId,
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
    },
    /// Provision intent is durable. JIT is not stored here.
    Provisioning {
        /// Provision intent.
        intent: ProvisionIntentId,
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
    },
    /// Runner is up and can take a job.
    IdleAssignable {
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
    },
    /// A job is running.
    Running {
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
    },
    /// GitHub conclusion observed. Cleanup is separate.
    Finishing {
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
    },
    /// Owned objects are being removed.
    Cleaning {
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
        /// Objects that must be deleted.
        owned: OwnedIds,
    },
    /// Ownership failed. Capacity stays held.
    Quarantined {
        /// Epoch allowed to mutate this slot.
        epoch: Epoch,
        /// Why deletion stopped.
        reason: OwnershipFailure,
    },
    /// Cleanup was proven. The slot does not count.
    Released,
}

impl WorkerState {
    /// Epoch stamped on this worker, if it still exists.
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

    /// Released workers do not consume a permit.
    #[must_use]
    pub const fn counts_capacity(&self) -> bool {
        !matches!(self, Self::Released)
    }
}

/// What the caller must do after a legal transition. Nothing here performs I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// State advanced. Capacity is unchanged.
    Advance,
    /// Same observation again. Do not mint another grant.
    Idempotent,
    /// Cleanup proof covered the owned ids. Capacity may be released.
    ReleaseCapacity,
    /// Keep the permit (quarantine or uncertainty).
    KeepCapacity,
}

/// Planned next state plus the effect the caller must apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// State to store after the effect is recorded.
    pub next: WorkerState,
    /// Effect the caller applies. This function did not apply it.
    pub effect: Effect,
}

/// Input to [`transition`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerEvent {
    /// Begin acquire for an already reserved slot.
    AcquireStarted {
        /// Acquire intent.
        intent: AcquireIntentId,
        /// Epoch of the caller.
        epoch: Epoch,
    },
    /// Service returned the requested ids.
    Acquired {
        /// Epoch of the caller.
        epoch: Epoch,
    },
    /// Transport outcome is unknown.
    AcquireUncertain {
        /// Epoch of the caller.
        epoch: Epoch,
    },
    /// Provision intent is durable. JIT comes after this, elsewhere.
    ProvisionStarted {
        /// Provision intent.
        intent: ProvisionIntentId,
        /// Epoch of the caller.
        epoch: Epoch,
    },
    /// Runner is idle.
    Idle {
        /// Epoch of the caller.
        epoch: Epoch,
    },
    /// Runner accepted a job.
    Running {
        /// Epoch of the caller.
        epoch: Epoch,
    },
    /// Job reached a terminal GitHub conclusion.
    Finishing {
        /// Epoch of the caller.
        epoch: Epoch,
    },
    /// Cleanup has started.
    Cleaning {
        /// Epoch of the caller.
        epoch: Epoch,
        /// Objects recorded before deletion.
        owned: OwnedIds,
    },
    /// Owned objects are gone.
    CleanupVerified {
        /// Epoch of the caller.
        epoch: Epoch,
        /// Proof the owned ids are gone.
        proof: CleanupProof,
    },
    /// Immutable id did not match.
    OwnershipMismatch {
        /// Epoch of the caller.
        epoch: Epoch,
        /// Why the object was refused.
        reason: OwnershipFailure,
    },
    /// The same request was delivered again.
    Redelivered {
        /// Request that was seen again.
        id: RequestId,
    },
}

/// Plan the next state. A stale epoch is rejected and must not be stored.
///
/// # Errors
///
/// Returns [`StateError::StaleEpoch`] or [`StateError::IllegalTransition`].
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

#[cfg(test)]
mod tests;
