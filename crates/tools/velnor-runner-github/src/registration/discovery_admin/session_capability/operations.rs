use std::collections::BTreeSet;
use std::time::SystemTime;

use zeroize::Zeroize;

use crate::EncodedJit;
use crate::policy::{ParsedTrustBatch, PollWithTrust, VerifiedJobTrust};
use crate::refresh::RefreshGate;
use crate::session::{
    SessionError, SessionRequest, acquire, create_session, jit, jit_request, poll_with_trust_route,
    refresh_queue_request,
};
use crate::{AcquireOutcome, DiscoveryTransport, InnerKind, ParsedBatch, WireError};

use super::VerifiedPoolSessionAdmin;
use super::acquire::refresh_acquire_and_route;
use super::origin::{bind_admin_origin, bind_queue_origin};
use super::types::{
    AcquireUnresolvedReason, PopulationObservationSource, SessionPopulationObservation,
    VerifiedAcquireOutcome, VerifiedAcquiredJob, VerifiedQueueSession,
};

impl VerifiedPoolSessionAdmin {
    /// Create one queue session after binding the capability's exact admin
    /// origin. The host transport validates and binds the returned full queue
    /// URL and returns its typed path/query route. The admin token never leaves
    /// this wrapper.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe session or transport error. A failure to validate
    /// the queue URL after the create request is treated as uncertain.
    pub fn create_session<T>(
        &mut self,
        transport: &mut T,
        owner: &str,
    ) -> Result<VerifiedQueueSession, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        self.require_fresh()?;
        if self.session_creation_attempted {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        bind_admin_origin(transport, self)?;
        // Create is a one-shot effect. Any error after dispatch leaves this
        // capability unable to send a second POST that could create a sibling
        // session or obscure the first response.
        self.session_creation_attempted = true;
        let raw = create_session(
            transport,
            self.binding.scale_set_id,
            owner,
            self.connection.expose_token(),
        )?;
        self.created_session_id = Some(raw.session_id.clone());
        let queue_route = transport
            .bind_message_queue_origin(&raw.message_queue_url)
            .map_err(|_| SessionError::Uncertain)?;
        let population_observation = raw.statistics().cloned().map(|statistics| {
            SessionPopulationObservation::new(
                raw.session_id.clone(),
                self.binding.scale_set_id,
                None,
                PopulationObservationSource::SessionCreated,
                SystemTime::now(),
                statistics,
            )
        });
        Ok(VerifiedQueueSession {
            inner: raw,
            queue_route,
            scale_set_id: self.binding.scale_set_id,
            policy_digest: self.policy_digest.clone(),
            available_requests: BTreeSet::new(),
            unrequested_requests: BTreeSet::new(),
            unresolved_requests: BTreeSet::new(),
            acquired_requests: BTreeSet::new(),
            completed_requests: BTreeSet::new(),
            unresolved_available: false,
            last_message_id: None,
            last_batch: None,
            population_observation,
            active_assigned_demand: None,
            next_assigned_demand_id: 1,
            one_shot: super::types::QueueOneShotState::default(),
        })
    }

    /// Bind the queue origin, then poll and bind the returned immutable trust
    /// batch to this exact session and Scale Set. On 401 the host transport
    /// validates and binds the refreshed queue URL before replay.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe session or transport error. The previous batch's
    /// offers are invalidated before the poll; an error does not authorize ACK.
    pub fn poll_with_trust<T>(
        &self,
        transport: &mut T,
        session: &mut VerifiedQueueSession,
        cursor: i64,
        total_capacity: u32,
        gate: &RefreshGate,
    ) -> Result<PollWithTrust, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        self.require_session(session)?;
        if session.last_batch.is_some()
            || session.active_assigned_demand.is_some()
            || session.one_shot.assigned_jit_uncertain
        {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        bind_queue_origin(transport, session)?;
        let queue_token = session.inner.token().to_owned();
        let initial_route = session.queue_route.duplicate();
        let set_id = self.binding.scale_set_id;
        let session_id = session.inner.session_id.clone();
        // A failed next poll must not leave the previous batch eligible for
        // acquisition through this session capability.
        session.available_requests.clear();
        session.unrequested_requests.clear();
        session.unresolved_requests.clear();
        session.acquired_requests.clear();
        session.completed_requests.clear();
        session.unresolved_available = false;
        session.last_message_id = None;
        session.population_observation = None;
        session.active_assigned_demand = None;
        session.one_shot.ack_attempted = false;
        let result = poll_with_trust_route(
            transport,
            &initial_route,
            cursor,
            total_capacity,
            &queue_token,
            gate,
            |transport, request| refresh_and_route(transport, self, session, request, cursor),
        );
        let mut queue_token = queue_token;
        queue_token.zeroize();
        match result? {
            PollWithTrust::Empty => Ok(PollWithTrust::Empty),
            PollWithTrust::Batch(mut batch) => {
                if !batch.bind_session_context(&session_id, set_id) {
                    return Err(SessionError::Wire(WireError::Malformed));
                }
                let observed_at = SystemTime::now();
                session.available_requests = unique_available_requests(&batch);
                session.unresolved_available = batch
                    .events()
                    .iter()
                    .filter(|event| matches!(event.job().kind, InnerKind::Available))
                    .count()
                    != session.available_requests.len();
                session.last_message_id = Some(batch.message_id());
                session.population_observation = batch.statistics().cloned().map(|statistics| {
                    SessionPopulationObservation::new(
                        session_id.clone(),
                        set_id,
                        Some(batch.message_id()),
                        PopulationObservationSource::PollBatch,
                        observed_at,
                        statistics,
                    )
                });
                session.last_batch = Some(ParsedBatch {
                    message_id: batch.message_id(),
                    statistics: batch.statistics().cloned(),
                    jobs: batch
                        .events()
                        .iter()
                        .map(|event| event.job().clone())
                        .collect(),
                });
                Ok(PollWithTrust::Batch(batch))
            }
        }
    }

    /// Bind the Actions Service origin and acquire exactly one request carried
    /// by this session's most recent poll. Acquire uses the session queue token
    /// as its bearer, but the pinned protocol routes its URL through the
    /// Actions Service URL. A response that omits the ID is unresolved. The
    /// request is consumed locally before the call so this capability cannot
    /// replay Acquire after an uncertain result. After a token refresh, the
    /// callback validates and caches the returned queue URL for later polls
    /// and ACKs, then this capability restores the Actions Service origin for
    /// the Acquire replay.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe session or transport error. On error the request
    /// remains consumed locally and must not be retried through this capability.
    pub fn acquire_verified<T>(
        &self,
        transport: &mut T,
        session: &mut VerifiedQueueSession,
        trust: VerifiedJobTrust,
        gate: &RefreshGate,
    ) -> Result<VerifiedAcquireOutcome, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        self.require_fresh()?;
        self.require_session(session)?;
        if !session.acquired_requests.is_empty()
            || !session.unresolved_requests.is_empty()
            || session.one_shot.assigned_jit_uncertain
        {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        let (message_id, request_id) = self.verify_acquire_identity(session, &trust)?;
        bind_admin_origin(transport, self)?;
        if !session.available_requests.remove(&request_id) {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        session.unresolved_requests.insert(request_id);
        session.population_observation = None;
        let queue_token = session.inner.token().to_owned();
        let set_id = self.binding.scale_set_id;
        let result = acquire(
            transport,
            set_id,
            &[request_id],
            &[],
            &queue_token,
            gate,
            |transport, request| refresh_acquire_and_route(transport, self, session, request),
        );
        let mut queue_token = queue_token;
        queue_token.zeroize();
        match result? {
            AcquireOutcome::Acquired(ids)
                if ids.as_slice() == std::slice::from_ref(&request_id) =>
            {
                session.unresolved_requests.remove(&request_id);
                session.acquired_requests.insert(request_id);
                Ok(VerifiedAcquireOutcome::Acquired(Box::new(
                    VerifiedAcquiredJob {
                        trust,
                        session_id: session.inner.session_id.clone(),
                        scale_set_id: set_id,
                        policy_digest: self.policy_digest.clone(),
                    },
                )))
            }
            AcquireOutcome::Acquired(_) => Ok(VerifiedAcquireOutcome::Unresolved {
                message_id,
                request_id,
                reason: AcquireUnresolvedReason::Omitted,
            }),
            AcquireOutcome::Noop => Ok(VerifiedAcquireOutcome::Unresolved {
                message_id,
                request_id,
                reason: AcquireUnresolvedReason::Noop,
            }),
        }
    }

    /// Bind the admin origin and generate JIT only for the exact acquired
    /// event and session capability. This does not claim runner-job affinity.
    ///
    /// # Errors
    ///
    /// Returns a secret-safe session or transport error when the proof has
    /// expired, the acquired capability belongs to another session, or the
    /// service rejects the JIT request.
    pub fn jit_verified<T>(
        &self,
        transport: &mut T,
        session: &mut VerifiedQueueSession,
        acquired: VerifiedAcquiredJob,
        runner_name: &str,
    ) -> Result<EncodedJit, SessionError>
    where
        T: DiscoveryTransport + ?Sized,
    {
        self.require_fresh()?;
        self.require_session(session)?;
        let VerifiedAcquiredJob {
            trust,
            session_id,
            scale_set_id,
            policy_digest,
        } = acquired;
        if session_id != session.inner.session_id
            || scale_set_id != self.binding.scale_set_id
            || policy_digest != self.policy_digest
            || trust.source_session_id() != Some(session.inner.session_id.as_str())
            || trust.source_scale_set_id() != Some(self.binding.scale_set_id)
            || !session
                .acquired_requests
                .contains(&trust.runner_request_id())
        {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        let request = jit_request(runner_name).map_err(SessionError::from)?;
        bind_admin_origin(transport, self)?;
        let request_id = trust.runner_request_id();
        session.population_observation = None;
        let encoded = jit(
            transport,
            self.binding.scale_set_id,
            self.connection.expose_token(),
            &request,
        )?;
        session.acquired_requests.remove(&request_id);
        session.completed_requests.insert(request_id);
        Ok(encoded)
    }

    pub(super) fn verify_acquire_identity(
        &self,
        session: &VerifiedQueueSession,
        trust: &VerifiedJobTrust,
    ) -> Result<(i64, i64), SessionError> {
        let message_id = trust.message_id();
        let request_id = trust.runner_request_id();
        if trust.policy_digest() != self.policy_digest
            || !trust
                .repository_full_name()
                .eq_ignore_ascii_case(&self.binding.repository_full_name)
            || trust.source_session_id() != Some(session.inner.session_id.as_str())
            || trust.source_scale_set_id() != Some(self.binding.scale_set_id)
            || session.last_message_id != Some(message_id)
            || request_id <= 0
            || !session.available_requests.contains(&request_id)
        {
            return Err(SessionError::Wire(WireError::RegistrationRejected));
        }
        Ok((message_id, request_id))
    }
}

pub(super) fn refresh_and_route<T>(
    transport: &mut T,
    capability: &VerifiedPoolSessionAdmin,
    session: &mut VerifiedQueueSession,
    request: &mut SessionRequest,
    cursor: i64,
) -> Result<(), SessionError>
where
    T: DiscoveryTransport + ?Sized,
{
    bind_admin_origin(transport, capability)?;
    let queue_url = refresh_queue_request(
        transport,
        capability.binding.scale_set_id,
        &mut session.inner,
        capability.connection.expose_token(),
        request,
    )?;
    let refreshed_route = transport.bind_message_queue_origin(queue_url)?;
    request.replace_target(
        refreshed_route.path().to_owned(),
        refreshed_route.poll_query(cursor),
    );
    session.queue_route.replace_with(refreshed_route);
    Ok(())
}

fn unique_available_requests(batch: &ParsedTrustBatch) -> BTreeSet<i64> {
    let mut counts = std::collections::BTreeMap::<i64, usize>::new();
    for event in batch.events() {
        if let Some(request_id) = event.job().request_id.filter(|id| *id > 0) {
            *counts.entry(request_id).or_default() += 1;
        }
    }
    batch
        .events()
        .iter()
        .filter_map(|event| {
            (matches!(event.job().kind, InnerKind::Available))
                .then_some(event.job().request_id.filter(|id| *id > 0))
                .flatten()
        })
        .filter(|request_id| counts.get(request_id) == Some(&1))
        .collect()
}
