//! Journal-backed intents for one-shot GitHub discovery credentials.

use velnor_runner_github::{
    AsyncDiscoveryIntentStore, AsyncScopedDiscoveryIntentStore,
    DiscoveryCredentialOutcome as GithubOutcome, DiscoveryCredentialStep as GithubStep,
    DiscoveryIntentId, DiscoveryStoreFuture, RegistrationScope, SessionError,
};
use velnor_runner_journal::journal::{
    DiscoveryCredentialOutcome as JournalOutcome, DiscoveryCredentialScope as JournalScope,
    DiscoveryCredentialStep as JournalStep, Journal,
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
        if step == GithubStep::OrganizationRegistrationToken {
            return Box::pin(async { Err(SessionError::Uncertain) });
        }
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

impl AsyncScopedDiscoveryIntentStore for JournalDiscoveryIntentStore {
    fn persist_scope_before<'a>(
        &'a mut self,
        step: GithubStep,
        scope: RegistrationScope<'a>,
        target_repository_id: i64,
        target_repository_full_name: &'a str,
    ) -> DiscoveryStoreFuture<'a, DiscoveryIntentId> {
        let Ok(scope) = journal_scope(scope) else {
            return Box::pin(async { Err(SessionError::Uncertain) });
        };
        let journal = self.journal.clone();
        let step = journal_step(step);
        let full_name = target_repository_full_name.to_owned();
        Box::pin(async move {
            let row = journal
                .begin_scoped_discovery_credential_intent(
                    step,
                    scope,
                    target_repository_id,
                    &full_name,
                )
                .await
                .map_err(|_| SessionError::Uncertain)?;
            let stable_id = u64::try_from(row).map_err(|_| SessionError::Uncertain)?;
            DiscoveryIntentId::new(stable_id).ok_or(SessionError::Uncertain)
        })
    }
}

const fn journal_step(step: GithubStep) -> JournalStep {
    match step {
        GithubStep::RepositoryRegistrationToken => JournalStep::RepositoryRegistrationToken,
        GithubStep::OrganizationRegistrationToken => JournalStep::OrganizationRegistrationToken,
        GithubStep::ActionsAdminExchange => JournalStep::ActionsAdminExchange,
    }
}

fn journal_scope(scope: RegistrationScope<'_>) -> Result<JournalScope<'_>, ()> {
    match scope {
        RegistrationScope::Repository { owner, repo } => Ok(JournalScope::Repository {
            owner,
            repository: repo,
        }),
        RegistrationScope::Organization { org } => {
            Ok(JournalScope::Organization { organization: org })
        }
        RegistrationScope::Enterprise { .. } => Err(()),
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
