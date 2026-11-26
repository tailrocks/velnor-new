// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::process::ExitStatus;

use tokio::sync::oneshot;

use super::cleanup::CleanupProof;
use super::source::LaunchGrant;
use crate::rust_runner::ReservationId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Closed {
    Draining,
    ConfigChanged,
    MachineUnavailable,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Admission {
    Closed(Closed),
    Open { free: u32 },
}

pub(super) enum JobExit {
    Exited(ExitStatus),
    Stopped,
    SpawnFailed,
    AcquireFailed,
}

impl JobExit {
    pub(super) fn describe(&self) -> String {
        match self {
            Self::Exited(status) => format!("exited({status})"),
            Self::Stopped => "stopped".to_owned(),
            Self::SpawnFailed => "spawn_failed".to_owned(),
            Self::AcquireFailed => "acquire_failed".to_owned(),
        }
    }
}

pub(super) enum Phase {
    Launching {
        grant: LaunchGrant,
    },
    Running {
        stop: Option<oneshot::Sender<()>>,
        grant: LaunchGrant,
    },
    Proving {
        exit: JobExit,
        attempting: bool,
    },
}

pub(super) struct Slot {
    pub(super) connection: usize,
    pub(super) phase: Phase,
}

pub(super) struct Slots {
    capacity: u32,
    entries: BTreeMap<ReservationId, Slot>,
}

impl Slots {
    pub(super) const fn new(capacity: u32) -> Self {
        Self {
            capacity,
            entries: BTreeMap::new(),
        }
    }

    pub(super) fn free(&self) -> u32 {
        self.capacity
            .saturating_sub(u32::try_from(self.entries.len()).unwrap_or(u32::MAX))
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn take(
        &mut self,
        reservation: ReservationId,
        connection: usize,
        grant: LaunchGrant,
    ) -> bool {
        if self.free() == 0 {
            return false;
        }
        self.entries.insert(
            reservation,
            Slot {
                connection,
                phase: Phase::Launching { grant },
            },
        );
        true
    }

    pub(super) fn abandon_launch(&mut self, reservation: ReservationId) {
        if matches!(
            self.entries.get(&reservation),
            Some(Slot {
                phase: Phase::Launching { .. },
                ..
            })
        ) {
            self.entries.remove(&reservation);
        }
    }

    pub(super) fn start_running(
        &mut self,
        reservation: ReservationId,
        stop: oneshot::Sender<()>,
    ) -> bool {
        let Some(slot) = self.entries.get_mut(&reservation) else {
            return false;
        };
        let placeholder = Phase::Proving {
            exit: JobExit::SpawnFailed,
            attempting: false,
        };
        match std::mem::replace(&mut slot.phase, placeholder) {
            Phase::Launching { grant } => {
                slot.phase = Phase::Running {
                    stop: Some(stop),
                    grant,
                };
                true
            }
            other => {
                slot.phase = other;
                false
            }
        }
    }

    pub(super) fn begin_proof(
        &mut self,
        reservation: ReservationId,
        exit: JobExit,
    ) -> Option<LaunchGrant> {
        let slot = self.entries.get_mut(&reservation)?;
        let previous = std::mem::replace(
            &mut slot.phase,
            Phase::Proving {
                exit,
                attempting: false,
            },
        );
        match previous {
            Phase::Launching { grant } | Phase::Running { grant, .. } => Some(grant),
            Phase::Proving { .. } => None,
        }
    }

    pub(super) fn stop_all(&mut self) {
        for slot in self.entries.values_mut() {
            if let Phase::Running { stop, .. } = &mut slot.phase
                && let Some(stop) = stop.take()
            {
                let _ = stop.send(());
            }
        }
    }

    pub(super) fn attempting(&self) -> bool {
        self.entries.values().any(|slot| {
            matches!(
                slot.phase,
                Phase::Proving {
                    attempting: true,
                    ..
                }
            )
        })
    }

    pub(super) fn settled(&self) -> bool {
        self.entries
            .values()
            .all(|slot| matches!(slot.phase, Phase::Proving { .. }))
    }

    pub(super) fn claim_due_proofs(&mut self) -> Vec<(ReservationId, usize)> {
        let mut due = Vec::new();
        for (reservation, slot) in &mut self.entries {
            if let Phase::Proving { attempting, .. } = &mut slot.phase
                && !*attempting
            {
                *attempting = true;
                due.push((*reservation, slot.connection));
            }
        }
        due
    }

    pub(super) fn proof_failed(&mut self, reservation: ReservationId) {
        if let Some(Slot {
            phase: Phase::Proving { attempting, .. },
            ..
        }) = self.entries.get_mut(&reservation)
        {
            *attempting = false;
        }
    }

    pub(super) fn release(&mut self, proof: CleanupProof) -> Option<JobExit> {
        match self.entries.remove(&proof.into_reservation())?.phase {
            Phase::Proving { exit, .. } => Some(exit),
            Phase::Launching { .. } | Phase::Running { .. } => None,
        }
    }
}
