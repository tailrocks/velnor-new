//! Session IDs stay scoped to their Scale Set; unresolved creates block restart.

use velnor_runner_github::QueueSession;

use crate::error::HostError;
use crate::journal::{
    ConnectionJournal, HeaderAllocation, HeaderDeleteAttempt, HeaderPollAttempt, SessionCreateState,
};
use crate::listen::{Link, OWNER_NAME, Secret, annotate};
use crate::scale_set::EnsureError;

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
    use crate::journal::{CapacityLimits, Journal};

    use super::CompletedHeaderPoll;

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
}
