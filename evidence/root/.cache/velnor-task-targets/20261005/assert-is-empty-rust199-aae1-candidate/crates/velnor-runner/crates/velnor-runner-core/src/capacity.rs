//! One host-wide capacity authority. `Drop` does not free a slot.

use std::collections::BTreeMap;

use crate::error::StateError;
use crate::identity::{AcquireIntentId, Epoch, GrantId, WorkerId};
use crate::lifecycle::WorkerState;
use crate::ownership::CleanupProof;

/// One occupied or released slot.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Slot {
    state: WorkerState,
    grant: GrantId,
    intent: AcquireIntentId,
    first_seen: u64,
    tie_break: u64,
}

/// Host-wide permits. Every non-released lifecycle counts toward `max`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capacity {
    max: u32,
    next_grant: u64,
    slots: BTreeMap<WorkerId, Slot>,
}

impl Capacity {
    /// Empty authority. `max` is the total permit count, not free slots.
    #[must_use]
    pub const fn new(max: u32) -> Self {
        Self {
            max,
            next_grant: 1,
            slots: BTreeMap::new(),
        }
    }

    /// Permits currently held.
    #[must_use]
    pub fn occupancy(&self) -> usize {
        self.slots
            .values()
            .filter(|slot| slot.state.counts_capacity())
            .count()
    }

    /// Reserve one permit. Replaying `intent` returns the original grant.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::CapacityExhausted`] when occupancy would exceed `max`,
    /// or [`StateError::IllegalTransition`] when the grant counter wraps.
    pub fn reserve(
        &mut self,
        worker: WorkerId,
        intent: AcquireIntentId,
        epoch: Epoch,
        first_seen: u64,
        tie_break: u64,
    ) -> Result<GrantId, StateError> {
        if let Some(slot) = self.slots.values().find(|slot| slot.intent == intent) {
            return Ok(slot.grant);
        }
        if u32::try_from(self.occupancy()).unwrap_or(u32::MAX) >= self.max {
            return Err(StateError::CapacityExhausted);
        }
        let grant = GrantId::new(self.next_grant).map_err(|_| StateError::IllegalTransition)?;
        self.next_grant = self.next_grant.saturating_add(1);
        self.slots.insert(
            worker,
            Slot {
                state: WorkerState::Reserved { grant, epoch },
                grant,
                intent,
                first_seen,
                tie_break,
            },
        );
        Ok(grant)
    }

    /// Record a redelivery. First-seen order does not change.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::UnknownWorker`] when `worker` is absent.
    pub fn note_redelivery(&mut self, worker: WorkerId) -> Result<(), StateError> {
        self.slots
            .get(&worker)
            .map(|_| ())
            .ok_or(StateError::UnknownWorker)
    }

    /// Admission order: first-seen, then the immutable tie-break.
    #[must_use]
    pub fn admission_order(&self) -> Vec<WorkerId> {
        let mut rows: Vec<(&WorkerId, &Slot)> = self
            .slots
            .iter()
            .filter(|(_, slot)| slot.state.counts_capacity())
            .collect();
        rows.sort_by_key(|(_, slot)| (slot.first_seen, slot.tie_break));
        rows.into_iter().map(|(id, _)| *id).collect()
    }

    /// Replace the stored state. Does not release capacity by itself.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::UnknownWorker`] when `worker` is absent.
    pub fn store(&mut self, worker: WorkerId, state: WorkerState) -> Result<(), StateError> {
        let slot = self
            .slots
            .get_mut(&worker)
            .ok_or(StateError::UnknownWorker)?;
        slot.state = state;
        Ok(())
    }

    /// Release only when cleanup proof covers the cleaning slot.
    ///
    /// # Errors
    ///
    /// Returns [`StateError::UnknownWorker`], [`StateError::IllegalTransition`],
    /// or [`StateError::CleanupMismatch`].
    pub fn release(&mut self, worker: WorkerId, proof: &CleanupProof) -> Result<(), StateError> {
        let slot = self
            .slots
            .get_mut(&worker)
            .ok_or(StateError::UnknownWorker)?;
        let WorkerState::Cleaning { owned, .. } = &slot.state else {
            return Err(StateError::IllegalTransition);
        };
        if !proof.covers(owned) {
            return Err(StateError::CleanupMismatch);
        }
        slot.state = WorkerState::Released;
        Ok(())
    }

    /// Borrow the worker state.
    #[must_use]
    pub fn state(&self, worker: WorkerId) -> Option<&WorkerState> {
        self.slots.get(&worker).map(|slot| &slot.state)
    }
}
