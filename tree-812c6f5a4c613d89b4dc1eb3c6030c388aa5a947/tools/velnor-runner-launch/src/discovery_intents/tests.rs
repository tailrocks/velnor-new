use velnor_runner_github::{
    AsyncDiscoveryIntentStore, DiscoveryCredentialOutcome, DiscoveryCredentialStep, SessionError,
};
use velnor_runner_journal::{IntentState, Journal, Outcome};

use super::JournalDiscoveryIntentStore;

#[tokio::test]
async fn pending_credential_intent_survives_reopen_and_blocks_replay() {
    let scratch =
        crate::launch::harness::Scratch::new("discovery-intent-replay").expect("scratch directory");
    let path = scratch.file();
    let journal = Journal::open(&path).await.expect("open journal");
    let mut store = JournalDiscoveryIntentStore::new(&journal);
    let first = store
        .persist_before(
            DiscoveryCredentialStep::RepositoryRegistrationToken,
            731,
            "Acme/Old",
        )
        .await
        .expect("durable first intent");
    assert!(first.get() > 0);
    assert_eq!(
        journal
            .finish(
                i64::try_from(first.get()).expect("journal id"),
                Outcome::DefiniteFailure
            )
            .await,
        Err(velnor_runner_journal::HostError::Journal)
    );
    assert_eq!(journal.rows().await.expect("rows").len(), 1);
    drop(store);
    drop(journal);

    let reopened = Journal::open(&path).await.expect("reopen journal");
    let mut store = JournalDiscoveryIntentStore::new(&reopened);
    let replay = store
        .persist_before(
            DiscoveryCredentialStep::RepositoryRegistrationToken,
            731,
            "acme/old",
        )
        .await;
    assert_eq!(replay, Err(SessionError::Uncertain));
    let renamed_replay = store
        .persist_before(
            DiscoveryCredentialStep::RepositoryRegistrationToken,
            731,
            "Acme/New",
        )
        .await;
    assert_eq!(renamed_replay, Err(SessionError::Uncertain));
    let rows = reopened.rows().await.expect("persisted intent");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Pending);
}

#[tokio::test]
async fn discovery_intents_are_independent_by_repository_id_and_step() {
    let scratch =
        crate::launch::harness::Scratch::new("discovery-intent-scopes").expect("scratch directory");
    let journal = Journal::open(&scratch.file()).await.expect("open journal");
    let mut store = JournalDiscoveryIntentStore::new(&journal);
    let first = store
        .persist_before(
            DiscoveryCredentialStep::RepositoryRegistrationToken,
            731,
            "Acme/Old",
        )
        .await
        .expect("first scope intent");
    let other_step = store
        .persist_before(
            DiscoveryCredentialStep::ActionsAdminExchange,
            731,
            "Acme/Old",
        )
        .await
        .expect("different operation has independent intent");
    let other_repository = store
        .persist_before(
            DiscoveryCredentialStep::RepositoryRegistrationToken,
            732,
            "Acme/Other",
        )
        .await
        .expect("different immutable repository ID has independent intent");

    assert_ne!(first.get(), other_step.get());
    assert_ne!(first.get(), other_repository.get());
    assert_eq!(journal.rows().await.expect("rows").len(), 3);
    assert_eq!(
        store
            .persist_before(
                DiscoveryCredentialStep::RepositoryRegistrationToken,
                731,
                "Acme/New",
            )
            .await,
        Err(SessionError::Uncertain)
    );
    assert_eq!(journal.rows().await.expect("rows").len(), 3);
}

#[tokio::test]
async fn credential_outcome_is_exact_and_cannot_clear_uncertainty() {
    let scratch = crate::launch::harness::Scratch::new("discovery-intent-outcome")
        .expect("scratch directory");
    let journal = Journal::open(&scratch.file()).await.expect("open journal");
    let mut store = JournalDiscoveryIntentStore::new(&journal);
    let id = store
        .persist_before(
            DiscoveryCredentialStep::ActionsAdminExchange,
            732,
            "Acme/RunnerRepo",
        )
        .await
        .expect("durable exchange intent");
    store
        .record_outcome(id, DiscoveryCredentialOutcome::Succeeded)
        .await
        .expect("record success");
    store
        .record_outcome(id, DiscoveryCredentialOutcome::Succeeded)
        .await
        .expect("same outcome is idempotent");
    assert_eq!(
        store
            .record_outcome(id, DiscoveryCredentialOutcome::Uncertain)
            .await,
        Err(SessionError::Uncertain)
    );
    let rows = journal.rows().await.expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, IntentState::Done);
}
