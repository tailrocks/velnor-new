use std::collections::BTreeSet;
use std::fmt;
use std::time::SystemTime;

use crate::policy::VerifiedJobTrust;
use crate::{MessageQueueRoute, ParsedBatch, Statistics, session::QueueSession};

/// Result of one single-request Acquire attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum VerifiedAcquireOutcome {
    /// The exact requested ID was returned by `AcquireJobs`.
    Acquired(Box<VerifiedAcquiredJob>),
    /// The service omitted the requested ID, or reported it already acquired.
    /// The result is unresolved: callers must not retry Acquire or mint JIT.
    Unresolved {
        /// Queue message that carried the offer.
        message_id: i64,
        /// The unique runner request ID from that event.
        request_id: i64,
        /// Non-effect certainty for an omitted or already-acquired response.
        reason: AcquireUnresolvedReason,
    },
}

/// Nonterminal Acquire response categories. Neither proves a retry is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireUnresolvedReason {
    /// A successful response omitted the requested singleton ID.
    Omitted,
    /// The pinned client returned its `Noop` classification.
    Noop,
}

/// Source of one scope-bound service population snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopulationObservationSource {
    /// Statistics returned when this exact queue session was created.
    SessionCreated,
    /// Statistics returned with this exact poll message.
    PollBatch,
}

/// Outcome of the single exact-session close attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCloseOutcome {
    /// The exact owned session received HTTP 204 and was retired locally.
    Closed,
    /// The session still has an open batch or unresolved request; no request
    /// was sent, and the same capability may continue that batch.
    Held,
}

/// A bounded amount of generic runner demand derived from one exact current
/// assigned-population observation. It is not bound to a job, request, run, or
/// runner; a verified homogeneous pool policy is what makes a generic JIT
/// runner eligible for the work the service may assign to it.
#[must_use]
pub struct VerifiedAssignedDemand {
    pub(super) demand_id: u64,
    pub(super) session_id: String,
    pub(super) scale_set_id: i64,
    pub(super) policy_digest: String,
    pub(super) message_id: Option<i64>,
    pub(super) source: PopulationObservationSource,
    pub(super) observed_at: SystemTime,
    pub(super) assigned_jobs: u32,
    pub(super) running_jobs: u32,
    pub(super) remaining_jit_count: u32,
    pub(super) next_slot_ordinal: u32,
}

impl VerifiedAssignedDemand {
    /// Queue session that supplied the exact population snapshot.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Scale Set covered by the verified pool policy.
    #[must_use]
    pub const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    /// Message ID when the source was a poll response; absent for session-create
    /// statistics. It is audit context, not a job identity.
    #[must_use]
    pub const fn message_id(&self) -> Option<i64> {
        self.message_id
    }

    /// Source of the assigned-population snapshot.
    #[must_use]
    pub const fn source(&self) -> PopulationObservationSource {
        self.source
    }

    /// Local receipt time of the exact statistics response.
    #[must_use]
    pub const fn observed_at(&self) -> SystemTime {
        self.observed_at
    }

    /// Assigned job count from the response, including running jobs.
    #[must_use]
    pub const fn assigned_jobs(&self) -> u32 {
        self.assigned_jobs
    }

    /// Running job count from the same response.
    #[must_use]
    pub const fn running_jobs(&self) -> u32 {
        self.running_jobs
    }

    /// Remaining one-shot generic JIT permits, bounded by both observed
    /// assigned-minus-running demand and the caller's free capacity.
    #[must_use]
    pub const fn remaining_jit_count(&self) -> u32 {
        self.remaining_jit_count
    }

    /// Monotonic demand identity within this exact session capability.
    #[must_use]
    pub const fn demand_id(&self) -> u64 {
        self.demand_id
    }

    /// Ordinal of the next one-shot JIT slot. Persist this exact ordinal before
    /// calling [`VerifiedPoolSessionAdmin::jit_assigned_demand`]. A dispatched
    /// attempt consumes the ordinal even if its response is uncertain.
    #[must_use]
    pub const fn next_slot_ordinal(&self) -> u32 {
        self.next_slot_ordinal
    }
}

impl fmt::Debug for VerifiedAssignedDemand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedAssignedDemand")
            .field("demand_id", &self.demand_id)
            .field("session_id", &self.session_id)
            .field("scale_set_id", &self.scale_set_id)
            .field("message_id", &self.message_id)
            .field("source", &self.source)
            .field("observed_at", &self.observed_at)
            .field("assigned_jobs", &self.assigned_jobs)
            .field("running_jobs", &self.running_jobs)
            .field("remaining_jit_count", &self.remaining_jit_count)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ActiveAssignedDemand {
    pub(super) demand_id: u64,
    pub(super) remaining_jit_count: u32,
}

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct QueueOneShotState {
    pub(super) assigned_jit_uncertain: bool,
    pub(super) ack_attempted: bool,
    pub(super) close_attempted: bool,
}

/// Fresh statistics attached to one exact Scale Set session generation.
/// This is an observation, not proof that the population is trusted or empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPopulationObservation {
    session_id: String,
    scale_set_id: i64,
    message_id: Option<i64>,
    source: PopulationObservationSource,
    observed_at: SystemTime,
    statistics: Statistics,
}

impl SessionPopulationObservation {
    pub(super) fn new(
        session_id: String,
        scale_set_id: i64,
        message_id: Option<i64>,
        source: PopulationObservationSource,
        observed_at: SystemTime,
        statistics: Statistics,
    ) -> Self {
        Self {
            session_id,
            scale_set_id,
            message_id,
            source,
            observed_at,
            statistics,
        }
    }

    /// Queue session that supplied this snapshot.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Actions Service Scale Set queried by the session.
    #[must_use]
    pub const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    /// Message ID when this snapshot came from a poll; create-time stats have
    /// no message identity.
    #[must_use]
    pub const fn message_id(&self) -> Option<i64> {
        self.message_id
    }

    /// Exact protocol response that supplied this snapshot.
    #[must_use]
    pub const fn source(&self) -> PopulationObservationSource {
        self.source
    }

    /// Local receipt time for the response.
    #[must_use]
    pub const fn observed_at(&self) -> SystemTime {
        self.observed_at
    }

    /// Counts returned by the Actions Service.
    #[must_use]
    pub const fn statistics(&self) -> &Statistics {
        &self.statistics
    }
}

/// Queue session whose set, pool proof, and last polled offers are immutable
/// bindings. The queue token remains private and zeroizes with `QueueSession`.
#[must_use]
pub struct VerifiedQueueSession {
    pub(super) inner: QueueSession,
    pub(super) queue_route: MessageQueueRoute,
    pub(super) scale_set_id: i64,
    pub(super) policy_digest: String,
    pub(super) available_requests: BTreeSet<i64>,
    pub(super) unrequested_requests: BTreeSet<i64>,
    pub(super) unresolved_requests: BTreeSet<i64>,
    pub(super) acquired_requests: BTreeSet<i64>,
    pub(super) completed_requests: BTreeSet<i64>,
    pub(super) unresolved_available: bool,
    pub(super) last_message_id: Option<i64>,
    pub(super) last_batch: Option<ParsedBatch>,
    pub(super) population_observation: Option<SessionPopulationObservation>,
    pub(super) active_assigned_demand: Option<ActiveAssignedDemand>,
    pub(super) next_assigned_demand_id: u64,
    pub(super) one_shot: QueueOneShotState,
}

impl VerifiedQueueSession {
    /// Protocol session ID, not a runner or job ID.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.inner.session_id
    }

    /// Last service message ID delivered by the verified polling method.
    #[must_use]
    pub const fn last_message_id(&self) -> Option<i64> {
        self.last_message_id
    }

    /// Latest exact-session statistics, if the create or current poll response
    /// included them. Any subsequent poll or job-side effect invalidates this
    /// snapshot. It does not establish that the observed population is trusted
    /// or that admission is safe.
    #[must_use]
    pub const fn population_observation(&self) -> Option<&SessionPopulationObservation> {
        self.population_observation.as_ref()
    }
}

impl fmt::Debug for VerifiedQueueSession {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedQueueSession")
            .field("session", &self.inner)
            .field("queue_route", &self.queue_route)
            .field("scale_set_id", &self.scale_set_id)
            .field("policy_digest", &self.policy_digest)
            .field("last_message_id", &self.last_message_id)
            .field("population_observation", &self.population_observation)
            .field(
                "assigned_jit_uncertain",
                &self.one_shot.assigned_jit_uncertain,
            )
            .field(
                "assigned_demand_remaining",
                &self
                    .active_assigned_demand
                    .map(|demand| demand.remaining_jit_count),
            )
            .field("close_attempted", &self.one_shot.close_attempted)
            .field("available_request_count", &self.available_requests.len())
            .field(
                "unrequested_request_count",
                &self.unrequested_requests.len(),
            )
            .field("unresolved_request_count", &self.unresolved_requests.len())
            .field("acquired_request_count", &self.acquired_requests.len())
            .field("completed_request_count", &self.completed_requests.len())
            .field("unresolved_available", &self.unresolved_available)
            .field("ack_attempted", &self.one_shot.ack_attempted)
            .finish_non_exhaustive()
    }
}

/// Exact single request whose `AcquireJobs` response returned that request ID.
/// It is non-Clone and can be consumed by one verified JIT call.
#[must_use]
#[derive(PartialEq, Eq)]
pub struct VerifiedAcquiredJob {
    pub(super) trust: VerifiedJobTrust,
    pub(super) session_id: String,
    pub(super) scale_set_id: i64,
    pub(super) policy_digest: String,
}

impl VerifiedAcquiredJob {
    /// Queue message ID that carried this request.
    #[must_use]
    pub const fn message_id(&self) -> i64 {
        self.trust.message_id()
    }

    /// Request ID returned exactly by `AcquireJobs`.
    #[must_use]
    pub const fn request_id(&self) -> i64 {
        self.trust.runner_request_id()
    }

    /// Workflow run ID retained from the verified event and REST lookup.
    #[must_use]
    pub const fn workflow_run_id(&self) -> i64 {
        self.trust.workflow_run_id()
    }

    /// Opaque protocol job ID, distinct from the Actions REST numeric job ID.
    #[must_use]
    pub fn scale_set_job_id(&self) -> Option<&str> {
        self.trust.scale_set_job_id()
    }

    /// Session ID used for the poll and Acquire call.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Actions Service Set ID used for the Acquire call.
    #[must_use]
    pub const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }
}

impl fmt::Debug for VerifiedAcquiredJob {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedAcquiredJob")
            .field("message_id", &self.message_id())
            .field("request_id", &self.request_id())
            .field("workflow_run_id", &self.workflow_run_id())
            .field("session_id", &self.session_id)
            .field("scale_set_id", &self.scale_set_id)
            .field("scale_set_job_id", &self.trust.scale_set_job_id())
            .finish_non_exhaustive()
    }
}
