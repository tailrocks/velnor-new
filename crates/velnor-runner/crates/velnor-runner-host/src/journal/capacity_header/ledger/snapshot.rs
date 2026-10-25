use std::collections::{BTreeMap, BTreeSet};

use super::super::model::{GenerationState, HeaderLedgerError, HeaderLedgerSnapshot};
use super::HeaderLedger;
use super::validation::valid_record;

impl HeaderLedger {
    /// Copy a deterministic snapshot for durable journal storage.
    #[must_use]
    pub(crate) fn snapshot(&self) -> HeaderLedgerSnapshot {
        HeaderLedgerSnapshot {
            max_jobs: self.max_jobs,
            next_generation: self.next_generation,
            records: self.records.values().cloned().collect(),
        }
    }

    /// Restore and validate persisted state; an interrupted poll stays charged.
    pub(crate) fn restore(snapshot: HeaderLedgerSnapshot) -> Result<Self, HeaderLedgerError> {
        Self::restore_with_poll_recovery(snapshot, true)
    }

    /// Restore a validated snapshot inside a running process transaction.
    ///
    /// Unlike [`Self::restore`], this keeps a persisted in-flight poll so its
    /// owning request can durably settle it. Startup recovery must use
    /// [`Self::restore`] to mark an interrupted request uncertain.
    pub(crate) fn restore_live(snapshot: HeaderLedgerSnapshot) -> Result<Self, HeaderLedgerError> {
        Self::restore_with_poll_recovery(snapshot, false)
    }

    fn restore_with_poll_recovery(
        mut snapshot: HeaderLedgerSnapshot,
        recover_interrupted_polls: bool,
    ) -> Result<Self, HeaderLedgerError> {
        if snapshot.max_jobs == 0 || snapshot.next_generation == 0 {
            return Err(HeaderLedgerError::InvalidSnapshot);
        }
        let mut records = BTreeMap::new();
        let mut generations = BTreeSet::new();
        let mut largest_generation = 0;
        for mut record in snapshot.records.drain(..) {
            if !valid_record(&record) || !generations.insert(record.generation) {
                return Err(HeaderLedgerError::InvalidSnapshot);
            }
            largest_generation = largest_generation.max(record.generation.0);
            if recover_interrupted_polls {
                if record.in_flight_poll.take().is_some() {
                    record.state = GenerationState::Uncertain;
                    record.zero_header_settled = false;
                }
                if record.in_flight_delete.take().is_some() {
                    record.delete_uncertain = true;
                }
            }
            if records.insert(record.identity.clone(), record).is_some() {
                return Err(HeaderLedgerError::InvalidSnapshot);
            }
        }
        if snapshot.next_generation <= largest_generation {
            return Err(HeaderLedgerError::InvalidSnapshot);
        }
        let ledger = Self {
            max_jobs: snapshot.max_jobs,
            next_generation: snapshot.next_generation,
            records,
        };
        if ledger.checked_charged_capacity()? > ledger.max_jobs
            || ledger.has_multiple_live_sessions_per_scale_set()
        {
            return Err(HeaderLedgerError::InvalidSnapshot);
        }
        Ok(ledger)
    }
}
