//! Session IDs stay scoped to their Scale Set; unresolved creates block restart.

use std::fs;
use std::path::{Path, PathBuf};

use bollard::models::SystemInfo;
use velnor_runner_github::{QueueSession, Transport, delete_session};

use crate::daemon_lock::{DaemonLock, EngineLineageGuard, canonical_journal_path};
use crate::error::HostError;
use crate::journal::{
    ConnectionJournal, EngineContext, HeaderAllocation, HeaderDeleteAttempt, HeaderPollAttempt,
    Journal, SessionCreateState,
};
use crate::listen::{Link, OWNER_NAME, Secret, annotate};
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
    let nonce = match active.first() {
        Some(record) if record.state() == SessionCreateState::Prepared => {
            record.local_nonce().to_owned()
        }
        Some(record)
            if matches!(
                record.state(),
                SessionCreateState::Requested
                    | SessionCreateState::Uncertain
                    | SessionCreateState::Bound
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
        .record_created_session(&claim, &session.session_id, 0)
        .await
        .map_err(map_journal)?;
    Ok((session, bound.intent_id(), bound.allocation().clone()))
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
    use std::path::PathBuf;

    use crate::config::{
        ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
        SourcePolicyMode, WorkflowEvent,
    };
    use crate::connect::SetOwnership;
    use crate::journal::{CapacityLimits, ConnectionJournal, HeaderAllocation, Journal};
    use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

    use super::{
        CompletedHeaderPoll, SessionDeleteOutcome, delete_removed_connection_session_with,
    };

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
