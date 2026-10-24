//! Bind one observation request to the next settled poll for its session.

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::config::RepositoryConnection;
use crate::error::HostError;
use crate::launch::steady::PollObservation;

use super::super::super::wire::{SessionObservationReply, SessionObservationStatus};
use super::ControlState;
use super::session_polls::binding_sha256;
use super::setup_receipt::SetupReceiptRecord;

pub(super) const RECEIPT_MAX_AGE: Duration = Duration::from_secs(5);
pub(super) const REQUEST_MAX_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const TICKET_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
pub(super) struct SessionObservationCoordinator {
    pub(super) poll_sequence: u64,
    pub(super) config_generation: u64,
    pub(super) setup_revision: u64,
    pub(super) setup_receipts: BTreeMap<String, SetupReceiptRecord>,
    pub(super) tickets: BTreeMap<String, SessionObservationTicket>,
}

pub(super) struct PollCompletion<'a> {
    pub(super) connection_id: &'a str,
    pub(super) poll_sequence: Option<u64>,
    pub(super) binding_sha256: &'a str,
    pub(super) scale_set_id: i64,
    pub(super) session_intent_id: i64,
    pub(super) session_id: &'a str,
    pub(super) poll: Option<PollObservation>,
}

#[derive(Debug)]
pub(super) struct SessionObservationTicket {
    pub(super) nonce: String,
    pub(super) binding_sha256: String,
    pub(super) config_generation: u64,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) scale_set_id: i64,
    pub(super) session_intent_id: i64,
    pub(super) session_id: String,
    pub(super) baseline_poll_sequence: u64,
    pub(super) deadline: Instant,
    pub(super) expires_at: Instant,
    pub(super) setup_revision: Option<u64>,
    pub(super) outcome: TicketOutcome,
}

#[derive(Debug)]
pub(super) enum TicketOutcome {
    Pending,
    Complete {
        poll: PollObservation,
        observed_at_unix_ms: u64,
    },
    Unavailable {
        observed_at_unix_ms: u64,
    },
    Stale,
    TimedOut,
}

impl SessionObservationCoordinator {
    pub(super) fn begin_poll(&mut self) -> Result<u64, HostError> {
        self.poll_sequence = self
            .poll_sequence
            .checked_add(1)
            .ok_or(HostError::Journal)?;
        Ok(self.poll_sequence)
    }

    pub(super) const fn poll_sequence(&self) -> u64 {
        self.poll_sequence
    }

    pub(super) fn replace_config(
        &mut self,
        bindings: &[crate::config::RepositoryConnection],
    ) -> Result<(), HostError> {
        self.config_generation = self
            .config_generation
            .checked_add(1)
            .ok_or(HostError::Journal)?;
        self.invalidate_all();
        self.setup_receipts.clear();
        self.tickets.retain(|connection_id, _| {
            bindings
                .iter()
                .any(|binding| binding.connection_id.as_str() == connection_id)
        });
        Ok(())
    }

    pub(super) fn invalidate_connection(&mut self, connection_id: &str) {
        if let Some(ticket) = self.tickets.get_mut(connection_id) {
            ticket.outcome = TicketOutcome::Stale;
        }
    }

    pub(super) fn invalidate_all(&mut self) {
        for ticket in self.tickets.values_mut() {
            ticket.outcome = TicketOutcome::Stale;
        }
    }

    pub(super) fn finish_poll(&mut self, completion: PollCompletion<'_>) {
        let Some(ticket) = self.tickets.get_mut(completion.connection_id) else {
            return;
        };
        if !matches!(&ticket.outcome, TicketOutcome::Pending) {
            return;
        }
        let Some(poll_sequence) = completion.poll_sequence else {
            ticket.outcome = TicketOutcome::Stale;
            return;
        };
        if Instant::now() >= ticket.deadline {
            ticket.outcome = TicketOutcome::TimedOut;
            return;
        }
        if poll_sequence <= ticket.baseline_poll_sequence
            || ticket.binding_sha256 != completion.binding_sha256
            || ticket.scale_set_id != completion.scale_set_id
            || ticket.session_intent_id != completion.session_intent_id
            || ticket.session_id != completion.session_id
        {
            ticket.outcome = TicketOutcome::Stale;
            return;
        }
        match completion.poll {
            Some(PollObservation::Unavailable) => {
                ticket.outcome = TicketOutcome::Unavailable {
                    observed_at_unix_ms: unix_time_ms(),
                };
            }
            Some(poll) => {
                ticket.outcome = TicketOutcome::Complete {
                    poll,
                    observed_at_unix_ms: unix_time_ms(),
                };
            }
            None => ticket.outcome = TicketOutcome::Stale,
        }
    }

    pub(super) fn remove_expired(&mut self, now: Instant) {
        self.tickets.retain(|_, ticket| now < ticket.expires_at);
    }
}

impl ControlState {
    pub(super) fn ticket_matches_current(
        &self,
        coordinator: &SessionObservationCoordinator,
        ticket: &SessionObservationTicket,
        connection_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
        config_generation: u64,
    ) -> Result<bool, HostError> {
        if ticket.config_generation != config_generation
            || ticket.engine_id != engine_id
            || ticket.docker_root_digest != docker_root_digest
        {
            return Ok(false);
        }
        let Some(binding) = self.current_binding(connection_id)? else {
            return Ok(false);
        };
        let digest = binding_sha256(&binding)?;
        let poll_matches = self.current_receipt_identity(connection_id, &digest)?;
        if let Some(revision) = ticket.setup_revision {
            let Some(setup) = coordinator.setup_receipts.get(connection_id) else {
                return Ok(false);
            };
            if setup.snapshot.revision != revision
                || !setup.matches(&digest, config_generation, engine_id, docker_root_digest)
                || setup.scale_set_id != ticket.scale_set_id
                || setup.session_intent_id != ticket.session_intent_id
                || setup.session_id != ticket.session_id
            {
                return Ok(false);
            }
        }
        Ok(poll_matches.is_some_and(|receipt| {
            ticket.binding_sha256 == digest
                && ticket.scale_set_id == receipt.scale_set_id
                && ticket.session_intent_id == receipt.session_intent_id
                && ticket.session_id == receipt.session_id
        }))
    }

    pub(super) fn current_binding(
        &self,
        connection_id: &str,
    ) -> Result<Option<RepositoryConnection>, HostError> {
        Ok(self
            .bindings
            .read()
            .map_err(|_| HostError::Journal)?
            .iter()
            .find(|binding| binding.connection_id.as_str() == connection_id)
            .cloned())
    }

    pub(super) fn current_receipt(
        &self,
        connection_id: &str,
        binding_digest: &str,
        now: Instant,
    ) -> Result<Option<super::session_polls::SessionPollReceipt>, HostError> {
        let polls = self.session_polls.read().map_err(|_| HostError::Journal)?;
        Ok(polls.get(connection_id).and_then(|receipt| {
            (receipt.binding_sha256 == binding_digest
                && now
                    .checked_duration_since(receipt.observed_at)
                    .is_some_and(|age| age <= RECEIPT_MAX_AGE))
            .then(|| receipt.clone())
        }))
    }

    pub(super) fn current_receipt_identity(
        &self,
        connection_id: &str,
        binding_digest: &str,
    ) -> Result<Option<super::session_polls::SessionPollReceipt>, HostError> {
        let polls = self.session_polls.read().map_err(|_| HostError::Journal)?;
        Ok(polls.get(connection_id).and_then(|receipt| {
            (receipt.binding_sha256 == binding_digest).then(|| receipt.clone())
        }))
    }

    pub(super) fn poll_pending(
        &self,
        connection_id: &str,
        now: Instant,
    ) -> Result<bool, HostError> {
        let pending = self.pending_polls.read().map_err(|_| HostError::Journal)?;
        Ok(pending
            .get(connection_id)
            .is_some_and(|attempt| now <= attempt.deadline))
    }
}

impl SessionObservationTicket {
    pub(super) fn reply(&mut self, now: Instant) -> SessionObservationReply {
        if matches!(&self.outcome, TicketOutcome::Pending) && now >= self.deadline {
            self.outcome = TicketOutcome::TimedOut;
        }
        match &self.outcome {
            TicketOutcome::Pending => reply(SessionObservationStatus::Pending, None, None),
            TicketOutcome::Complete {
                poll,
                observed_at_unix_ms,
            } => reply(
                SessionObservationStatus::Complete,
                Some(poll.clone()),
                Some(*observed_at_unix_ms),
            ),
            TicketOutcome::Unavailable {
                observed_at_unix_ms,
            } => reply(
                SessionObservationStatus::Unavailable,
                None,
                Some(*observed_at_unix_ms),
            ),
            TicketOutcome::Stale => reply(SessionObservationStatus::Stale, None, None),
            TicketOutcome::TimedOut => reply(SessionObservationStatus::TimedOut, None, None),
        }
    }
}

pub(super) fn reply(
    status: SessionObservationStatus,
    poll: Option<PollObservation>,
    observed_at_unix_ms: Option<u64>,
) -> SessionObservationReply {
    SessionObservationReply {
        status,
        poll,
        observed_at_unix_ms,
    }
}

pub(super) fn valid_nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_engine(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn unix_time_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod invalidation_lock_tests {
    use super::super::ControlState;
    use super::super::session_polls::SessionPollReceipt;
    use std::sync::{Arc, TryLockError};
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn all_poll_invalidation_holds_coordinator_until_pending_polls_clear() {
        let state = Arc::new(ControlState::empty());
        let receipt_inserted = state.session_polls.write().is_ok_and(|mut polls| {
            polls.insert(
                "connection".to_owned(),
                SessionPollReceipt {
                    connection_id: "connection".to_owned(),
                    binding_sha256: "binding".to_owned(),
                    scale_set_id: 1,
                    session_intent_id: 1,
                    session_id: "session".to_owned(),
                    generation: 1,
                    observed_at: Instant::now(),
                },
            );
            true
        });
        assert!(receipt_inserted, "test receipt should be inserted");

        let pending_guard = state
            .pending_polls
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let reset_state = Arc::clone(&state);
        let reset = thread::spawn(move || reset_state.invalidate_session_polls());
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut reached_pending_clear = false;
        while Instant::now() < deadline {
            if state
                .session_polls
                .read()
                .is_ok_and(|polls| polls.is_empty())
            {
                reached_pending_clear = true;
                break;
            }
            thread::yield_now();
        }
        assert!(reached_pending_clear, "reset should clear receipts first");
        assert!(
            matches!(
                state.session_observation.try_lock(),
                Err(TryLockError::WouldBlock)
            ),
            "reset must retain the coordinator lock while clearing pending polls"
        );

        drop(pending_guard);
        assert!(matches!(reset.join(), Ok(Ok(()))));
        assert!(
            state
                .pending_polls
                .read()
                .is_ok_and(|pending| pending.is_empty())
        );
    }
}
