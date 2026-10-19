//! Session IDs stay scoped to their Scale Set; unresolved creates block restart.

use std::fs;
use std::path::{Path, PathBuf};

use bollard::models::SystemInfo;
use velnor_runner_github::{
    QueueSession, SessionError, Transport, delete_session, refresh_session,
};

use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::error::HostError;
use crate::journal::{
    ConnectionJournal, EngineContext, HeaderAllocation, HeaderDeleteAttempt, HeaderPollAttempt,
    Journal, SessionCreateState, SessionRefreshClaim,
};
use crate::listen::{Link, OWNER_NAME, Secret, annotate, validate_queue_url};
use crate::scale_set::EnsureError;

/// Evidence that one exact refresh generation returned the bound session ID.
///
/// Production construction stays closed until the refresh call, exact-ID
/// check, and queue-route validation share one adapter path.
#[derive(Debug)]
pub(crate) struct VerifiedSessionRefresh {
    session_intent_id: i64,
    session_id: String,
    generation: i64,
}

impl VerifiedSessionRefresh {
    fn after_response(
        claim: &SessionRefreshClaim,
        session: &QueueSession,
    ) -> Result<Self, EnsureError> {
        if session.session_id != claim.session_id() {
            return Err(EnsureError::AdmissionClosed);
        }
        Ok(Self {
            session_intent_id: claim.session_intent_id(),
            session_id: claim.session_id().to_owned(),
            generation: claim.generation(),
        })
    }

    pub(crate) fn session_intent_id(&self) -> i64 {
        self.session_intent_id
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn generation(&self) -> i64 {
        self.generation
    }

    #[cfg(test)]
    pub(crate) fn test_only(session_intent_id: i64, session_id: &str, generation: i64) -> Self {
        Self {
            session_intent_id,
            session_id: session_id.to_owned(),
            generation,
        }
    }
}

/// Evidence that a returned refresh call had an uncertain transport outcome.
///
/// Production construction stays closed until the exact `refresh_session`
/// result is classified by its source adapter.
#[derive(Debug)]
pub(crate) struct ReturnedSessionRefreshTransportError {
    session_intent_id: i64,
    session_id: String,
    generation: i64,
}

impl ReturnedSessionRefreshTransportError {
    fn from_returned_result(
        claim: &SessionRefreshClaim,
        result: &Result<QueueSession, SessionError>,
    ) -> Option<Self> {
        matches!(result, Err(SessionError::Uncertain)).then(|| Self {
            session_intent_id: claim.session_intent_id(),
            session_id: claim.session_id().to_owned(),
            generation: claim.generation(),
        })
    }

    pub(crate) fn session_intent_id(&self) -> i64 {
        self.session_intent_id
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn generation(&self) -> i64 {
        self.generation
    }

    #[cfg(test)]
    pub(crate) fn test_only(session_intent_id: i64, session_id: &str, generation: i64) -> Self {
        Self {
            session_intent_id,
            session_id: session_id.to_owned(),
            generation,
        }
    }
}

/// Startup proof for classifying interrupted session refresh requests.
///
/// The capability retains references to the exact daemon and engine locks.
/// It is consumed by the journal and carries no queue credential or URL.
#[derive(Debug)]
pub(crate) struct SessionRefreshStartupAuthority<'a> {
    daemon_lock: &'a DaemonLock,
    daemon_lock_path: PathBuf,
    journal_path: PathBuf,
    instance_id: String,
    engine_id: String,
    revision: u64,
    lineage_guard: &'a EngineLineageGuard,
}

impl SessionRefreshStartupAuthority<'_> {
    /// Recheck the held daemon lock and journal's attached engine lineage.
    pub(crate) async fn validate_lock_and_lineage(
        &self,
        journal: &Journal,
    ) -> Result<(), HostError> {
        if !self.daemon_lock.matches_path(&self.daemon_lock_path) {
            return Err(HostError::Lock);
        }
        journal
            .validate_startup_attachment(
                &self.journal_path,
                &self.instance_id,
                &self.engine_id,
                self.revision,
                self.lineage_guard,
            )
            .await
    }

    pub(crate) fn canonical_journal_path(&self) -> &Path {
        &self.journal_path
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn engine_id(&self) -> &str {
        &self.engine_id
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn lineage_guard(&self) -> &EngineLineageGuard {
        self.lineage_guard
    }
}

impl<'a> SessionRefreshStartupAuthority<'a> {
    /// Mint a one-shot authority after checking the retained daemon lock,
    /// current Docker identity, and attached journal lineage.
    pub(crate) async fn after_validated_startup(
        daemon_lock: &'a DaemonLock,
        state: &Path,
        lineage_guard: &'a EngineLineageGuard,
        journal: &Journal,
        selected_engine: &super::SelectedEngineContext,
        fresh_info: &SystemInfo,
    ) -> Result<Self, HostError> {
        let canonical_state = fs::canonicalize(state).map_err(|_| HostError::Path)?;
        if !fs::metadata(&canonical_state)
            .map_err(|_| HostError::Path)?
            .is_dir()
        {
            return Err(HostError::Path);
        }
        let daemon_lock_path = canonical_state.join("daemon.lock");
        if !daemon_lock.matches_path(&daemon_lock_path) {
            return Err(HostError::Lock);
        }

        let selection = EngineContext {
            engine_id: selected_engine.engine_id().to_owned(),
            endpoint: selected_engine.endpoint().to_owned(),
            platform: selected_engine.platform().to_owned(),
        };
        let current = super::SelectedEngineContext::capture(&selection, fresh_info)?;
        if current != *selected_engine {
            return Err(HostError::Ownership);
        }

        let journal_path = canonical_journal_path(&canonical_state.join("launch.db"))?;
        let instance_id = journal.instance_id().await?;
        let engine_id = journal.engine_id().await?;
        let revision = journal.revision().await?;
        if engine_id != selected_engine.engine_id() || lineage_guard.engine_id() != engine_id {
            return Err(HostError::Ownership);
        }
        lineage_guard.verify_lineage(&journal_path, &instance_id, revision)?;

        let authority = Self {
            daemon_lock,
            daemon_lock_path,
            journal_path,
            instance_id,
            engine_id,
            revision,
            lineage_guard,
        };
        authority.validate_lock_and_lineage(journal).await?;
        Ok(authority)
    }

    #[cfg(test)]
    pub(crate) fn test_only(
        daemon_lock: &'a DaemonLock,
        daemon_lock_path: &Path,
        journal_path: &Path,
        instance_id: &str,
        engine_id: &str,
        revision: u64,
        lineage_guard: &'a EngineLineageGuard,
    ) -> Result<Self, HostError> {
        Ok(Self {
            daemon_lock,
            daemon_lock_path: fs::canonicalize(daemon_lock_path).map_err(|_| HostError::Path)?,
            journal_path: canonical_journal_path(journal_path)?,
            instance_id: instance_id.to_owned(),
            engine_id: engine_id.to_owned(),
            revision,
            lineage_guard,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionDeleteOutcome {
    NotReady,
    Deleted,
    Held,
}

/// Proof that the exact allocated poll returned and its response was handled.
pub(crate) struct CompletedHeaderPoll {
    attempt: HeaderPollAttempt,
}

impl CompletedHeaderPoll {
    pub(super) fn after_response(
        attempt: HeaderPollAttempt,
        allocation: &HeaderAllocation,
        journal: &ConnectionJournal,
        scale_set_id: i64,
        session_id: &str,
    ) -> Result<Self, EnsureError> {
        if !allocation.matches_session(journal.connection_id(), scale_set_id, session_id)
            || !attempt.matches_session(journal.connection_id(), scale_set_id, session_id)
            || (attempt.header_value() != 0 && allocation.target() != attempt.header_value())
        {
            return Err(EnsureError::AdmissionClosed);
        }
        Ok(Self { attempt })
    }

    pub(crate) fn into_attempt(self) -> HeaderPollAttempt {
        self.attempt
    }

    #[cfg(test)]
    pub(crate) fn test_only(attempt: HeaderPollAttempt) -> Self {
        Self { attempt }
    }
}

/// Proof that GitHub returned the exact successful 204 for an owned session DELETE.
pub(crate) struct DeletedSession204 {
    attempt: HeaderDeleteAttempt,
}

impl DeletedSession204 {
    fn after_response(
        attempt: HeaderDeleteAttempt,
        allocation: &HeaderAllocation,
        journal: &ConnectionJournal,
        scale_set_id: i64,
        session_id: &str,
    ) -> Result<Self, EnsureError> {
        if !allocation.matches_session(journal.connection_id(), scale_set_id, session_id)
            || !attempt.matches_session(journal.connection_id(), scale_set_id, session_id)
        {
            return Err(EnsureError::AdmissionClosed);
        }
        Ok(Self { attempt })
    }

    pub(crate) fn into_attempt(self) -> HeaderDeleteAttempt {
        self.attempt
    }

    #[cfg(test)]
    pub(crate) fn test_only(attempt: HeaderDeleteAttempt) -> Self {
        Self { attempt }
    }
}

/// Create one scoped session after storing its create intent.
pub(crate) async fn open_connection_session(
    link: &mut Link,
    set_id: i64,
    journal: &ConnectionJournal,
    requested_header_capacity: u32,
) -> Result<(QueueSession, i64, HeaderAllocation), EnsureError> {
    let binding = journal
        .source_authorization_binding()
        .await
        .map_err(map_journal)?;
    let registered = journal
        .scale_set()
        .await
        .map_err(map_journal)?
        .ok_or(EnsureError::AdmissionClosed)?;
    if binding.scale_set_id() != set_id || registered.id != set_id {
        return Err(EnsureError::AdmissionClosed);
    }
    let records = journal.session_creates().await.map_err(map_journal)?;
    let active = records
        .iter()
        .filter(|record| {
            record.scale_set_id() == set_id
                && matches!(
                    record.state(),
                    SessionCreateState::Prepared
                        | SessionCreateState::Requested
                        | SessionCreateState::Uncertain
                        | SessionCreateState::Bound
                )
        })
        .collect::<Vec<_>>();
    if active.len() > 1 {
        return Err(EnsureError::Uncertain);
    }
    if let Some(record) = active
        .first()
        .filter(|record| record.state() == SessionCreateState::Bound)
    {
        let session_id = record.session_id().ok_or(EnsureError::AdmissionClosed)?;
        let session_intent_id = record.intent_id().ok_or(EnsureError::AdmissionClosed)?;
        let allocation = journal
            .session_header_allocation(session_intent_id)
            .await
            .map_err(map_journal)?;
        if allocation.connection_id() != journal.connection_id()
            || allocation.scale_set_id() != set_id
            || allocation.session_id() != session_id
        {
            return Err(EnsureError::AdmissionClosed);
        }
        let mut session =
            refresh_bound_connection_session(link, journal, session_intent_id, set_id, session_id)
                .await?;
        if allocation.target() == requested_header_capacity {
            return Ok((session, session_intent_id, allocation));
        }
        let polled = crate::launch::poll_scoped_connection_once(
            link,
            set_id,
            &mut session,
            journal,
            session_intent_id,
            &allocation,
            0,
        )
        .await?;
        if !polled.session_delete_candidate
            || !journal
                .session_work_drained(session_intent_id)
                .await
                .map_err(map_journal)?
        {
            return Err(EnsureError::AdmissionClosed);
        }
        if delete_removed_connection_session(
            link,
            &session,
            journal,
            session_intent_id,
            &allocation,
        )
        .await
            != SessionDeleteOutcome::Deleted
        {
            return Err(EnsureError::Uncertain);
        }
    }
    let nonce = match active.first() {
        Some(record) if record.state() == SessionCreateState::Prepared => {
            record.local_nonce().to_owned()
        }
        Some(record)
            if matches!(
                record.state(),
                SessionCreateState::Requested | SessionCreateState::Uncertain
            ) =>
        {
            // A lost create response or a bound session needs exact recovery
            // before another session can be opened.
            return Err(EnsureError::Uncertain);
        }
        Some(_) => return Err(EnsureError::AdmissionClosed),
        None => uuid::Uuid::new_v4().simple().to_string(),
    };
    let prepared = journal
        .prepare_session_create(&nonce)
        .await
        .map_err(map_journal)?;
    if prepared.state() != SessionCreateState::Prepared {
        return Err(EnsureError::Uncertain);
    }
    let claim = journal
        .claim_session_create(&nonce)
        .await
        .map_err(map_journal)?
        .ok_or(EnsureError::Uncertain)?;
    let admin_token = Secret::new(link.token());
    let session = match velnor_runner_github::create_session(
        link.transport(),
        set_id,
        OWNER_NAME,
        admin_token.expose(),
    ) {
        Ok(session) => session,
        Err(error) => {
            journal
                .mark_session_create_uncertain(&claim)
                .await
                .map_err(map_journal)?;
            return Err(annotate(error, "create-session"));
        }
    };
    let bound = journal
        .record_created_session(&claim, &session.session_id, requested_header_capacity)
        .await
        .map_err(map_journal)?;
    Ok((session, bound.intent_id(), bound.allocation().clone()))
}

/// Refresh one bound session after a durable generation claim.
///
/// The returned queue credentials stay in memory and are usable only after the
/// exact ID, queue route, and generation receipt all pass.
pub(crate) async fn refresh_bound_connection_session(
    link: &mut Link,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    scale_set_id: i64,
    expected_session_id: &str,
) -> Result<QueueSession, EnsureError> {
    let admin_token = Secret::new(link.token());
    let admin_base = link.base().to_owned();
    refresh_bound_connection_session_with(
        link.transport(),
        admin_token.expose(),
        &admin_base,
        scoped,
        session_intent_id,
        scale_set_id,
        expected_session_id,
    )
    .await
}

async fn refresh_bound_connection_session_with<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    admin_base: &str,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    scale_set_id: i64,
    expected_session_id: &str,
) -> Result<QueueSession, EnsureError> {
    let allocation = scoped
        .session_header_allocation(session_intent_id)
        .await
        .map_err(map_journal)?;
    if session_intent_id <= 0
        || scale_set_id <= 0
        || expected_session_id.is_empty()
        || allocation.connection_id() != scoped.connection_id()
        || allocation.scale_set_id() != scale_set_id
        || allocation.session_id() != expected_session_id
    {
        return Err(EnsureError::AdmissionClosed);
    }

    let claim = scoped
        .claim_session_refresh(session_intent_id, expected_session_id)
        .await
        .map_err(map_journal)?
        .ok_or(EnsureError::Uncertain)?;
    if claim.session_intent_id() != session_intent_id
        || claim.scale_set_id() != scale_set_id
        || claim.session_id() != expected_session_id
    {
        scoped
            .mark_session_refresh_uncertain(claim)
            .await
            .map_err(map_journal)?;
        return Err(EnsureError::AdmissionClosed);
    }

    let returned = refresh_session(transport, scale_set_id, expected_session_id, admin_token);
    let transport_proof =
        ReturnedSessionRefreshTransportError::from_returned_result(&claim, &returned);
    let session = match returned {
        Ok(session) => session,
        Err(error) => {
            if let Some(proof) = transport_proof {
                scoped
                    .record_session_refresh_transport_error(claim, proof)
                    .await
                    .map_err(map_journal)?;
            } else {
                scoped
                    .mark_session_refresh_uncertain(claim)
                    .await
                    .map_err(map_journal)?;
            }
            return Err(annotate(error, "refresh-session"));
        }
    };

    if session.session_id != expected_session_id {
        scoped
            .mark_session_refresh_uncertain(claim)
            .await
            .map_err(map_journal)?;
        return Err(EnsureError::AdmissionClosed);
    }
    if let Err(error) = validate_queue_url(admin_base, &session.message_queue_url) {
        scoped
            .mark_session_refresh_uncertain(claim)
            .await
            .map_err(map_journal)?;
        return Err(error);
    }
    let proof = match VerifiedSessionRefresh::after_response(&claim, &session) {
        Ok(proof) => proof,
        Err(error) => {
            scoped
                .mark_session_refresh_uncertain(claim)
                .await
                .map_err(map_journal)?;
            return Err(error);
        }
    };
    scoped
        .record_session_refreshed(claim, proof)
        .await
        .map_err(map_journal)?;
    Ok(session)
}

/// Delete only the exact bound session after its zero-header poll and local drain.
///
/// Any effect error or proof-write failure leaves the durable attempt in-flight or uncertain.
pub(crate) async fn delete_removed_connection_session(
    link: &mut Link,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
) -> SessionDeleteOutcome {
    let admin_token = Secret::new(link.token());
    delete_removed_connection_session_with(
        link.transport(),
        admin_token.expose(),
        session,
        scoped,
        session_intent_id,
        allocation,
    )
    .await
}

async fn delete_removed_connection_session_with<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
) -> SessionDeleteOutcome {
    if allocation.connection_id() != scoped.connection_id()
        || allocation.session_id() != session.session_id
    {
        return SessionDeleteOutcome::NotReady;
    }
    let attempt = match scoped
        .begin_session_delete(session_intent_id, allocation)
        .await
    {
        Ok(attempt) => attempt,
        Err(_) => return SessionDeleteOutcome::NotReady,
    };

    if delete_session(
        transport,
        allocation.scale_set_id(),
        allocation.session_id(),
        admin_token,
    )
    .is_err()
    {
        let _ignored = scoped
            .mark_session_delete_uncertain(session_intent_id, attempt)
            .await;
        return SessionDeleteOutcome::Held;
    }

    let proof = match DeletedSession204::after_response(
        attempt,
        allocation,
        scoped,
        allocation.scale_set_id(),
        &session.session_id,
    ) {
        Ok(proof) => proof,
        Err(_) => return SessionDeleteOutcome::Held,
    };
    match scoped
        .retire_session_after_delete(session_intent_id, proof)
        .await
    {
        Ok(()) => SessionDeleteOutcome::Deleted,
        Err(_) => SessionDeleteOutcome::Held,
    }
}

fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;

    use crate::config::{
        ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
        SourcePolicyMode, WorkflowEvent,
    };
    use crate::connect::SetOwnership;
    use crate::journal::{
        CapacityLimits, ConnectionJournal, HeaderAllocation, Journal, SessionRefreshState,
    };
    use velnor_runner_github::{
        Exchange, Method, Poll, RefreshGate, SessionRequest, Transport, TransportFail, WireError,
        poll,
    };

    use super::{
        CompletedHeaderPoll, SessionDeleteOutcome, delete_removed_connection_session_with,
        refresh_bound_connection_session_with,
    };

    #[path = "poll_refresh_tests.rs"]
    mod poll_refresh_tests;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Result<Self, String> {
            let path = std::env::temp_dir().join(format!(
                "velnor-header-zero-proof-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
            Ok(Self(path))
        }

        fn file(&self) -> PathBuf {
            self.0.join("journal.db")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ignored = std::fs::remove_dir_all(&self.0);
        }
    }

    fn binding() -> Result<RepositoryConnection, String> {
        let selector = ScaleSetSelector::try_new(
            "integration",
            &["velnor".to_owned(), "integration".to_owned()],
        )
        .map_err(|error| error.to_string())?;
        let policy = SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::Push],
            Vec::new(),
        )
        .map_err(|error| error.to_string())?;
        RepositoryConnection::try_new(
            ConnectionId::parse("header-zero-proof").map_err(|error| error.to_string())?,
            101,
            "owner/repository".to_owned(),
            KeychainReference::parse("keychain:velnor/header-zero-proof")
                .map_err(|error| error.to_string())?,
            selector,
            policy,
        )
        .map_err(|error| error.to_string())
    }

    struct DeleteTransport {
        status: u16,
        body: Vec<u8>,
        calls: Vec<(Method, String, Option<String>, Option<String>)>,
    }

    impl Transport for DeleteTransport {
        fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
            let authorization = request
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(_, value)| value.clone());
            self.calls.push((
                request.method,
                request.path.clone(),
                request.query.clone(),
                authorization,
            ));
            Ok(Exchange {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }

    struct RefreshTransport {
        responses: VecDeque<Result<Exchange, TransportFail>>,
        requests: Vec<SessionRequest>,
    }

    impl RefreshTransport {
        fn new(responses: impl IntoIterator<Item = Result<Exchange, TransportFail>>) -> Self {
            Self {
                responses: responses.into_iter().collect(),
                requests: Vec::new(),
            }
        }
    }

    impl Transport for RefreshTransport {
        fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
            self.requests.push(request.clone());
            self.responses
                .pop_front()
                .unwrap_or(Err(TransportFail::Reset))
        }
    }

    fn refreshed_body(session_id: &str, queue_url: &str, token: &str) -> Vec<u8> {
        format!(
            r#"{{"sessionId":"{session_id}","messageQueueUrl":"{queue_url}","messageQueueAccessToken":"{token}"}}"#
        )
        .into_bytes()
    }

    async fn bound_session(
        scratch: &Scratch,
        target: u32,
        session_id: &str,
    ) -> Result<(Journal, ConnectionJournal, i64, HeaderAllocation), String> {
        let journal = Journal::open(&scratch.file())
            .await
            .map_err(|error| error.to_string())?;
        journal
            .configure_capacity(CapacityLimits {
                jobs: 2,
                cpu_millicores: 2_000,
                memory_bytes: 2_147_483_648,
            })
            .await
            .map_err(|error| error.to_string())?;
        journal
            .bootstrap_header_authority()
            .await
            .map_err(|error| error.to_string())?;
        journal
            .bind_engine("test-engine")
            .await
            .map_err(|error| error.to_string())?;
        let binding = binding()?;
        let scope = journal
            .connection_scope(&binding)
            .await
            .map_err(|error| error.to_string())?;
        scope
            .bind_scale_set(10_101, "integration", SetOwnership::Adopted)
            .await
            .map_err(|error| error.to_string())?;
        let prepared = scope
            .prepare_session_create("fedcba9876543210fedcba9876543210")
            .await
            .map_err(|error| error.to_string())?;
        let claim = scope
            .claim_session_create(prepared.local_nonce())
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "session create claim missing".to_owned())?;
        let bound = scope
            .record_created_session(&claim, session_id, target)
            .await
            .map_err(|error| error.to_string())?;
        Ok((
            journal,
            scope,
            bound.intent_id(),
            bound.allocation().clone(),
        ))
    }

    #[tokio::test]
    async fn exact_refresh_receipt_builds_next_poll_with_new_memory_credentials()
    -> Result<(), String> {
        let scratch = Scratch::new()?;
        let session_id = "session-refresh-exact";
        let (journal, scope, intent_id, allocation) =
            bound_session(&scratch, 0, session_id).await?;
        let mut transport = RefreshTransport::new([
            Ok(Exchange {
                status: 200,
                body: refreshed_body(session_id, "messages-new", "queue-token-new"),
            }),
            Ok(Exchange {
                status: 202,
                body: Vec::new(),
            }),
        ]);

        let refreshed = refresh_bound_connection_session_with(
            &mut transport,
            "admin-token",
            "https://admin.example.test",
            &scope,
            intent_id,
            allocation.scale_set_id(),
            session_id,
        )
        .await
        .map_err(|error| error.to_string())?;
        let poll_result = poll(
            &mut transport,
            "messages-new",
            0,
            0,
            refreshed.token(),
            &RefreshGate::new(),
            || Ok::<(), WireError>(()),
        );

        assert!(matches!(poll_result, Ok(Poll::Empty)));
        assert_eq!(transport.requests.len(), 2);
        assert_eq!(transport.requests[0].method, Method::Patch);
        assert_eq!(
            transport.requests[0].path,
            format!(
                "_apis/runtime/runnerscalesets/{}/sessions/{session_id}",
                allocation.scale_set_id()
            )
        );
        assert!(transport.requests[0].headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value == "Bearer admin-token"
        }));
        assert_eq!(transport.requests[1].method, Method::Get);
        assert_eq!(transport.requests[1].path, "messages-new");
        assert!(transport.requests[1].headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value == "Bearer queue-token-new"
        }));
        let records = scope
            .session_refreshes(intent_id)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state(), SessionRefreshState::Succeeded);
        assert_eq!(
            journal
                .charged_header_capacity()
                .await
                .map_err(|error| error.to_string())?,
            0
        );
        Ok(())
    }

    #[tokio::test]
    async fn returned_transport_uncertainty_allows_only_a_new_durable_generation()
    -> Result<(), String> {
        let scratch = Scratch::new()?;
        let session_id = "session-refresh-retry";
        let (_journal, scope, intent_id, allocation) =
            bound_session(&scratch, 0, session_id).await?;
        let mut first = RefreshTransport::new([Err(TransportFail::Reset)]);
        let first_result = refresh_bound_connection_session_with(
            &mut first,
            "admin-token",
            "https://admin.example.test",
            &scope,
            intent_id,
            allocation.scale_set_id(),
            session_id,
        )
        .await;
        assert!(matches!(
            first_result,
            Err(crate::scale_set::EnsureError::Uncertain)
        ));
        assert_eq!(first.requests.len(), 1);

        let mut second = RefreshTransport::new([Ok(Exchange {
            status: 200,
            body: refreshed_body(session_id, "messages-next", "queue-token-next"),
        })]);
        let refreshed = refresh_bound_connection_session_with(
            &mut second,
            "admin-token",
            "https://admin.example.test",
            &scope,
            intent_id,
            allocation.scale_set_id(),
            session_id,
        )
        .await
        .map_err(|error| error.to_string())?;
        assert_eq!(refreshed.session_id, session_id);
        assert_eq!(second.requests.len(), 1);
        assert_eq!(second.requests[0].method, Method::Patch);
        let records = scope
            .session_refreshes(intent_id)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].generation(), 1);
        assert_eq!(records[0].state(), SessionRefreshState::TransportError);
        assert_eq!(records[1].generation(), 2);
        assert_eq!(records[1].state(), SessionRefreshState::Succeeded);
        Ok(())
    }

    #[tokio::test]
    async fn incorrect_id_or_queue_route_stays_uncertain_and_never_installs_credentials()
    -> Result<(), String> {
        for (name, returned_id, queue_url) in [
            ("incorrect-id", "foreign-session", "messages"),
            (
                "incorrect-route",
                "session-refresh-invalid",
                "http://queue.example/messages",
            ),
        ] {
            let scratch = Scratch::new()?;
            let session_id = if name == "incorrect-id" {
                "session-refresh-expected"
            } else {
                "session-refresh-invalid"
            };
            let (_journal, scope, intent_id, allocation) =
                bound_session(&scratch, 0, session_id).await?;
            let mut transport = RefreshTransport::new([Ok(Exchange {
                status: 200,
                body: refreshed_body(returned_id, queue_url, "must-not-install"),
            })]);
            let result = refresh_bound_connection_session_with(
                &mut transport,
                "admin-token",
                "https://admin.example.test",
                &scope,
                intent_id,
                allocation.scale_set_id(),
                session_id,
            )
            .await;

            assert!(result.is_err(), "{name} must not return queue credentials");
            assert_eq!(transport.requests.len(), 1);
            let records = scope
                .session_refreshes(intent_id)
                .await
                .map_err(|error| error.to_string())?;
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].state(), SessionRefreshState::Uncertain);
            assert!(
                scope
                    .claim_session_refresh(intent_id, session_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .is_none()
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn rejected_refresh_and_wrong_durable_id_send_no_retryable_request() -> Result<(), String>
    {
        let scratch = Scratch::new()?;
        let session_id = "session-refresh-rejected";
        let (_journal, scope, intent_id, allocation) =
            bound_session(&scratch, 0, session_id).await?;
        let mut transport = RefreshTransport::new([Ok(Exchange {
            status: 409,
            body: Vec::new(),
        })]);
        let result = refresh_bound_connection_session_with(
            &mut transport,
            "admin-token",
            "https://admin.example.test",
            &scope,
            intent_id,
            allocation.scale_set_id(),
            session_id,
        )
        .await;
        assert!(matches!(
            result,
            Err(crate::scale_set::EnsureError::Conflict)
        ));
        assert_eq!(transport.requests.len(), 1);
        let records = scope
            .session_refreshes(intent_id)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(records[0].state(), SessionRefreshState::Uncertain);
        assert!(
            scope
                .claim_session_refresh(intent_id, session_id)
                .await
                .map_err(|error| error.to_string())?
                .is_none()
        );

        let mut wrong_id_transport = RefreshTransport::new([]);
        assert!(
            refresh_bound_connection_session_with(
                &mut wrong_id_transport,
                "admin-token",
                "https://admin.example.test",
                &scope,
                intent_id,
                allocation.scale_set_id(),
                "different-session",
            )
            .await
            .is_err()
        );
        assert!(wrong_id_transport.requests.is_empty());
        Ok(())
    }

    async fn settle_zero_poll(
        scope: &ConnectionJournal,
        intent_id: i64,
        allocation: &HeaderAllocation,
        session_id: &str,
    ) -> Result<(), String> {
        let attempt = scope
            .begin_zero_header_poll(intent_id, allocation)
            .await
            .map_err(|error| error.to_string())?;
        let proof = CompletedHeaderPoll::after_response(
            attempt,
            allocation,
            scope,
            allocation.scale_set_id(),
            session_id,
        )
        .map_err(|error| error.to_string())?;
        scope
            .settle_session_poll(intent_id, proof)
            .await
            .map_err(|error| error.to_string())
    }

    #[tokio::test]
    async fn zero_header_attempt_produces_proof_without_releasing_positive_charge()
    -> Result<(), String> {
        let scratch = Scratch::new()?;
        let journal = Journal::open(&scratch.file())
            .await
            .map_err(|error| error.to_string())?;
        journal
            .configure_capacity(CapacityLimits {
                jobs: 1,
                cpu_millicores: 1_000,
                memory_bytes: 1_073_741_824,
            })
            .await
            .map_err(|error| error.to_string())?;
        journal
            .bootstrap_header_authority()
            .await
            .map_err(|error| error.to_string())?;
        journal
            .bind_engine("test-engine")
            .await
            .map_err(|error| error.to_string())?;
        let binding = binding()?;
        let scope = journal
            .connection_scope(&binding)
            .await
            .map_err(|error| error.to_string())?;
        scope
            .bind_scale_set(10_101, "integration", SetOwnership::Adopted)
            .await
            .map_err(|error| error.to_string())?;
        let prepared = scope
            .prepare_session_create("0123456789abcdef0123456789abcdef")
            .await
            .map_err(|error| error.to_string())?;
        let claim = scope
            .claim_session_create(prepared.local_nonce())
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "session create claim missing".to_owned())?;
        let bound = scope
            .record_created_session(&claim, "session-zero-proof", 1)
            .await
            .map_err(|error| error.to_string())?;
        let allocation = bound.allocation().clone();
        assert_eq!(allocation.target(), 1);

        let attempt = scope
            .begin_zero_header_poll(bound.intent_id(), &allocation)
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(attempt.header_value(), 0);
        let proof = CompletedHeaderPoll::after_response(
            attempt,
            &allocation,
            &scope,
            10_101,
            "session-zero-proof",
        )
        .map_err(|error| error.to_string())?;
        scope
            .settle_session_poll(bound.intent_id(), proof)
            .await
            .map_err(|error| error.to_string())?;

        assert_eq!(
            journal
                .charged_header_capacity()
                .await
                .map_err(|error| error.to_string())?,
            1
        );
        let current = scope
            .session_header_allocation(bound.intent_id())
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(current.target(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn exact_204_deletes_only_the_bound_session_and_retires_its_charge() -> Result<(), String>
    {
        let scratch = Scratch::new()?;
        let session_id = "session-exact-delete";
        let (journal, scope, intent_id, allocation) =
            bound_session(&scratch, 1, session_id).await?;
        settle_zero_poll(&scope, intent_id, &allocation, session_id).await?;
        let session = velnor_runner_github::create_session(
            &mut DeleteTransport {
                status: 200,
                body: format!(
                    r#"{{"sessionId":"{session_id}","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token"}}"#
                )
                .into_bytes(),
                calls: Vec::new(),
            },
            allocation.scale_set_id(),
            "velnor-host",
            "unused-admin-token",
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(session.session_id, session_id);

        let mut transport = DeleteTransport {
            status: 204,
            body: Vec::new(),
            calls: Vec::new(),
        };
        let outcome = delete_removed_connection_session_with(
            &mut transport,
            "admin-token",
            &session,
            &scope,
            intent_id,
            &allocation,
        )
        .await;

        assert_eq!(outcome, SessionDeleteOutcome::Deleted);
        assert_eq!(
            transport.calls,
            vec![(
                Method::Delete,
                format!(
                    "_apis/runtime/runnerscalesets/{}/sessions/{session_id}",
                    allocation.scale_set_id()
                ),
                Some("api-version=6.0-preview".to_owned()),
                Some("Bearer admin-token".to_owned()),
            )]
        );
        assert!(scope.session_header_allocation(intent_id).await.is_err());
        assert_eq!(
            journal
                .charged_header_capacity()
                .await
                .map_err(|error| error.to_string())?,
            0
        );
        Ok(())
    }

    #[tokio::test]
    async fn delete_is_not_sent_while_durable_session_work_remains() -> Result<(), String> {
        let scratch = Scratch::new()?;
        let session_id = "session-with-pending-work";
        let (journal, scope, intent_id, allocation) =
            bound_session(&scratch, 1, session_id).await?;
        settle_zero_poll(&scope, intent_id, &allocation, session_id).await?;
        scope
            .store_completion_inbox(44, "pending lifecycle event")
            .await
            .map_err(|error| error.to_string())?;
        let session = velnor_runner_github::create_session(
            &mut DeleteTransport {
                status: 200,
                body: format!(
                    r#"{{"sessionId":"{session_id}","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token"}}"#
                )
                .into_bytes(),
                calls: Vec::new(),
            },
            allocation.scale_set_id(),
            "velnor-host",
            "unused-admin-token",
        )
        .map_err(|error| error.to_string())?;
        let mut transport = DeleteTransport {
            status: 204,
            body: Vec::new(),
            calls: Vec::new(),
        };

        assert_eq!(
            delete_removed_connection_session_with(
                &mut transport,
                "admin-token",
                &session,
                &scope,
                intent_id,
                &allocation,
            )
            .await,
            SessionDeleteOutcome::NotReady
        );
        assert!(transport.calls.is_empty());
        assert_eq!(
            journal
                .charged_header_capacity()
                .await
                .map_err(|error| error.to_string())?,
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn non_204_delete_holds_charge_and_does_not_retry() -> Result<(), String> {
        let scratch = Scratch::new()?;
        let session_id = "session-delete-uncertain";
        let (journal, scope, intent_id, allocation) =
            bound_session(&scratch, 1, session_id).await?;
        settle_zero_poll(&scope, intent_id, &allocation, session_id).await?;
        let session = velnor_runner_github::create_session(
            &mut DeleteTransport {
                status: 200,
                body: format!(
                    r#"{{"sessionId":"{session_id}","messageQueueUrl":"https://queue.example/messages","messageQueueAccessToken":"queue-token"}}"#
                )
                .into_bytes(),
                calls: Vec::new(),
            },
            allocation.scale_set_id(),
            "velnor-host",
            "unused-admin-token",
        )
        .map_err(|error| error.to_string())?;
        let mut transport = DeleteTransport {
            status: 200,
            body: Vec::new(),
            calls: Vec::new(),
        };
        assert_eq!(
            delete_removed_connection_session_with(
                &mut transport,
                "admin-token",
                &session,
                &scope,
                intent_id,
                &allocation,
            )
            .await,
            SessionDeleteOutcome::Held
        );
        assert_eq!(transport.calls.len(), 1);
        assert_eq!(
            journal
                .charged_header_capacity()
                .await
                .map_err(|error| error.to_string())?,
            1
        );
        assert_eq!(
            delete_removed_connection_session_with(
                &mut transport,
                "admin-token",
                &session,
                &scope,
                intent_id,
                &allocation,
            )
            .await,
            SessionDeleteOutcome::NotReady
        );
        assert_eq!(transport.calls.len(), 1);
        Ok(())
    }
}
