//! Journal-backed intents for one-shot GitHub discovery credentials.

use velnor_runner_github::{
    AsyncDiscoveryIntentStore, DiscoveryCredentialOutcome as GithubOutcome,
    DiscoveryCredentialStep as GithubStep, DiscoveryIntentId, DiscoveryStoreFuture, SessionError,
};
use velnor_runner_journal::journal::{
    DiscoveryCredentialOutcome as JournalOutcome, DiscoveryCredentialStep as JournalStep, Journal,
};

/// Durable adapter used before the two repository-discovery credential POSTs.
///
/// It stores only repository scope and operation state. Credentials and
/// response bodies remain in the GitHub helper's memory-only types.
#[derive(Debug, Clone)]
pub struct JournalDiscoveryIntentStore {
    journal: Journal,
}

impl JournalDiscoveryIntentStore {
    /// Bind a discovery store to an already-open journal.
    #[must_use]
    pub fn new(journal: &Journal) -> Self {
        Self {
            journal: journal.clone(),
        }
    }
}

impl AsyncDiscoveryIntentStore for JournalDiscoveryIntentStore {
    fn persist_before<'a>(
        &'a mut self,
        step: GithubStep,
        repository_id: i64,
        full_name: &'a str,
    ) -> DiscoveryStoreFuture<'a, DiscoveryIntentId> {
        let journal = self.journal.clone();
        let full_name = full_name.to_owned();
        let step = journal_step(step);
        Box::pin(async move {
            let row = journal
                .begin_discovery_credential_intent(step, repository_id, &full_name)
                .await
                .map_err(|_| SessionError::Uncertain)?;
            let stable_id = u64::try_from(row).map_err(|_| SessionError::Uncertain)?;
            DiscoveryIntentId::new(stable_id).ok_or(SessionError::Uncertain)
        })
    }

    fn record_outcome(
        &mut self,
        id: DiscoveryIntentId,
        outcome: GithubOutcome,
    ) -> DiscoveryStoreFuture<'_, ()> {
        let journal = self.journal.clone();
        let outcome = journal_outcome(outcome);
        Box::pin(async move {
            let row = i64::try_from(id.get()).map_err(|_| SessionError::Uncertain)?;
            journal
                .record_discovery_credential_outcome(row, outcome)
                .await
                .map_err(|_| SessionError::Uncertain)
        })
    }
}

const fn journal_step(step: GithubStep) -> JournalStep {
    match step {
        GithubStep::RepositoryRegistrationToken => JournalStep::RepositoryRegistrationToken,
        GithubStep::ActionsAdminExchange => JournalStep::ActionsAdminExchange,
    }
}

const fn journal_outcome(outcome: GithubOutcome) -> JournalOutcome {
    match outcome {
        GithubOutcome::Succeeded => JournalOutcome::Succeeded,
        GithubOutcome::Rejected => JournalOutcome::Rejected,
        GithubOutcome::Uncertain => JournalOutcome::Uncertain,
    }
}

#[cfg(test)]
#[path = "discovery_intents/tests.rs"]
mod tests;
