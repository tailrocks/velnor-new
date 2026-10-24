//! Bind one setup snapshot and fresh poll observation to a census request.

use std::time::{Duration, Instant};

use crate::controller_ipc::wire::{
    ConnectionCensusReply, ConnectionSetupSnapshot, SessionObservationReply,
    SessionObservationStatus, SetupReceiptStatus,
};
use crate::error::HostError;

use super::session_observation::{SessionObservationCoordinator, TicketOutcome};
use super::setup_receipt::SetupReceiptRecord;
use super::{ControlState, ObservationStart, binding_sha256, session_observation};

enum CensusSetup {
    Stale,
    Pending(Option<ConnectionSetupSnapshot>),
    Ready(SetupReceiptRecord),
}

fn census_reply(
    setup_status: SetupReceiptStatus,
    setup: Option<ConnectionSetupSnapshot>,
    observation: SessionObservationReply,
) -> ConnectionCensusReply {
    ConnectionCensusReply {
        setup_status,
        setup,
        observation,
    }
}

impl ControlState {
    pub(in crate::controller_ipc::server) fn start_connection_census(
        &self,
        connection_id: &str,
        nonce: &str,
        timeout: Duration,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<ConnectionCensusReply, HostError> {
        validate_start(nonce, timeout, engine_id, docker_root_digest)?;
        let now = Instant::now();
        let deadline = now.checked_add(timeout).ok_or(HostError::Identity)?;
        let expires_at = now
            .checked_add(session_observation::TICKET_TTL)
            .ok_or(HostError::Identity)?;
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        coordinator.remove_expired(now);
        let receipt = match self.census_setup(
            &coordinator,
            connection_id,
            engine_id,
            docker_root_digest,
            now,
        )? {
            CensusSetup::Stale => return Ok(stale_census()),
            CensusSetup::Pending(snapshot) => return Ok(pending_census(snapshot)),
            CensusSetup::Ready(receipt) => receipt,
        };
        let snapshot = receipt.snapshot.clone();
        let request = ObservationStart {
            connection_id,
            nonce,
            engine_id,
            docker_root_digest,
            now,
            deadline,
            expires_at,
            setup_revision: Some(snapshot.revision),
        };
        let observation =
            if let Some(reply) = self.start_existing_observation(&mut coordinator, &request)? {
                reply
            } else {
                self.start_new_observation(&mut coordinator, &request)?
            };
        Ok(census_reply(
            SetupReceiptStatus::Ready,
            Some(snapshot),
            observation,
        ))
    }

    fn census_setup(
        &self,
        coordinator: &SessionObservationCoordinator,
        connection_id: &str,
        engine_id: &str,
        docker_root_digest: &str,
        now: Instant,
    ) -> Result<CensusSetup, HostError> {
        if !self.admission_ready() {
            return Ok(CensusSetup::Stale);
        }
        let Some(binding) = self.current_binding(connection_id)? else {
            return Ok(CensusSetup::Stale);
        };
        let digest = binding_sha256(&binding)?;
        let Some(receipt) = coordinator.setup_receipts.get(connection_id) else {
            return Ok(CensusSetup::Pending(None));
        };
        if !receipt.matches(
            &digest,
            coordinator.config_generation,
            engine_id,
            docker_root_digest,
        ) {
            return Ok(CensusSetup::Stale);
        }
        if self.poll_pending(connection_id, now)? {
            return Ok(CensusSetup::Pending(Some(receipt.snapshot.clone())));
        }
        let Some(poll_receipt) = self.current_receipt(connection_id, &digest, now)? else {
            return Ok(CensusSetup::Pending(Some(receipt.snapshot.clone())));
        };
        if poll_receipt.scale_set_id != receipt.scale_set_id
            || poll_receipt.session_intent_id != receipt.session_intent_id
            || poll_receipt.session_id != receipt.session_id
        {
            return Ok(CensusSetup::Stale);
        }
        Ok(CensusSetup::Ready(receipt.clone()))
    }

    pub(in crate::controller_ipc::server) fn connection_census_status(
        &self,
        connection_id: &str,
        nonce: &str,
        engine_id: &str,
        docker_root_digest: &str,
    ) -> Result<ConnectionCensusReply, HostError> {
        if !session_observation::valid_nonce(nonce)
            || !session_observation::valid_engine(engine_id)
            || !session_observation::valid_digest(docker_root_digest)
        {
            return Err(HostError::Frame);
        }
        let now = Instant::now();
        let mut coordinator = self
            .session_observation
            .lock()
            .map_err(|_| HostError::Journal)?;
        coordinator.remove_expired(now);
        if !self.admission_ready() {
            coordinator.invalidate_connection(connection_id);
            return Ok(census_reply(
                SetupReceiptStatus::Stale,
                None,
                session_observation::reply(SessionObservationStatus::Stale, None, None),
            ));
        }
        let Some(ticket) = coordinator.tickets.get(connection_id) else {
            return Ok(census_reply(
                SetupReceiptStatus::Pending,
                None,
                session_observation::reply(SessionObservationStatus::Missing, None, None),
            ));
        };
        if ticket.nonce != nonce {
            return Ok(census_reply(
                SetupReceiptStatus::Pending,
                None,
                session_observation::reply(SessionObservationStatus::Missing, None, None),
            ));
        }
        let matches = self.ticket_matches_current(
            &coordinator,
            ticket,
            connection_id,
            engine_id,
            docker_root_digest,
            coordinator.config_generation,
        )? && ticket.setup_revision.is_some();
        let snapshot = if matches {
            ticket.setup_revision.and_then(|revision| {
                coordinator
                    .setup_receipts
                    .get(connection_id)
                    .filter(|setup| setup.snapshot.revision == revision)
                    .map(|setup| setup.snapshot.clone())
            })
        } else {
            None
        };
        let ticket = coordinator
            .tickets
            .get_mut(connection_id)
            .ok_or(HostError::Journal)?;
        if snapshot.is_none() {
            ticket.outcome = TicketOutcome::Stale;
        }
        let observation = ticket.reply(now);
        let setup_status = if snapshot.is_some() {
            SetupReceiptStatus::Ready
        } else {
            SetupReceiptStatus::Stale
        };
        Ok(census_reply(setup_status, snapshot, observation))
    }
}

fn validate_start(
    nonce: &str,
    timeout: Duration,
    engine_id: &str,
    docker_root_digest: &str,
) -> Result<(), HostError> {
    if !session_observation::valid_nonce(nonce)
        || timeout.is_zero()
        || timeout > session_observation::REQUEST_MAX_TIMEOUT
        || !session_observation::valid_engine(engine_id)
        || !session_observation::valid_digest(docker_root_digest)
    {
        return Err(HostError::Frame);
    }
    Ok(())
}

fn stale_census() -> ConnectionCensusReply {
    census_reply(
        SetupReceiptStatus::Stale,
        None,
        session_observation::reply(SessionObservationStatus::Stale, None, None),
    )
}

fn pending_census(snapshot: Option<ConnectionSetupSnapshot>) -> ConnectionCensusReply {
    let setup_status = if snapshot.is_some() {
        SetupReceiptStatus::Ready
    } else {
        SetupReceiptStatus::Pending
    };
    census_reply(
        setup_status,
        snapshot,
        session_observation::reply(SessionObservationStatus::Pending, None, None),
    )
}
