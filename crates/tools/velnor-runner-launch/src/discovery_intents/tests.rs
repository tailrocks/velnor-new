use velnor_runner_github::{
    AsyncDiscoveryIntentStore, AsyncScopedDiscoveryIntentStore, DiscoveryCredentialOutcome,
    DiscoveryCredentialStep, RegistrationScope, SessionError,
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
async fn legacy_repository_uncertainty_blocks_scoped_rename_after_reopen() {
    let scratch = crate::launch::harness::Scratch::new("discovery-legacy-to-scoped")
        .expect("scratch directory");
    let path = scratch.file();
    let journal = Journal::open(&path).await.expect("open journal");
    let mut store = JournalDiscoveryIntentStore::new(&journal);
    let id = store
        .persist_before(
            DiscoveryCredentialStep::RepositoryRegistrationToken,
            731,
            "Acme/Old",
        )
        .await
        .expect("persist legacy repository intent");
    store
        .record_outcome(id, DiscoveryCredentialOutcome::Uncertain)
        .await
        .expect("persist uncertain legacy outcome");
    drop(store);
    drop(journal);

    let reopened = Journal::open(&path).await.expect("reopen journal");
    let mut store = JournalDiscoveryIntentStore::new(&reopened);
    assert_eq!(
        store
            .persist_scope_before(
                DiscoveryCredentialStep::RepositoryRegistrationToken,
                RegistrationScope::Repository {
                    owner: "Acme",
                    repo: "Renamed",
                },
                731,
                "Acme/NewName",
            )
            .await,
        Err(SessionError::Uncertain),
        "switching API or renaming cannot escape a legacy immutable-ID fence"
    );
    assert_eq!(reopened.rows().await.expect("rows").len(), 1);
}

#[tokio::test]
async fn scoped_repository_intent_blocks_legacy_and_renamed_replays_after_reopen() {
    let scratch = crate::launch::harness::Scratch::new("discovery-scoped-to-legacy")
        .expect("scratch directory");
    let path = scratch.file();
    let journal = Journal::open(&path).await.expect("open journal");
    let mut store = JournalDiscoveryIntentStore::new(&journal);
    let scoped_id = store
        .persist_scope_before(
            DiscoveryCredentialStep::ActionsAdminExchange,
            RegistrationScope::Repository {
                owner: "Acme",
                repo: "RunnerRepo",
            },
            731,
            "Acme/RunnerRepo",
        )
        .await
        .expect("persist scoped repository intent");
    assert!(scoped_id.get() > 0);
    drop(store);
    drop(journal);

    let reopened = Journal::open(&path).await.expect("reopen journal");
    let mut store = JournalDiscoveryIntentStore::new(&reopened);
    assert_eq!(
        store
            .persist_before(
                DiscoveryCredentialStep::ActionsAdminExchange,
                731,
                "Acme/TransferredRepo",
            )
            .await,
        Err(SessionError::Uncertain),
        "legacy calls must observe scoped repository intents"
    );
    assert_eq!(
        store
            .persist_scope_before(
                DiscoveryCredentialStep::ActionsAdminExchange,
                RegistrationScope::Repository {
                    owner: "NewOwner",
                    repo: "MovedRepo",
                },
                731,
                "NewOwner/MovedRepo",
            )
            .await,
        Err(SessionError::Uncertain),
        "owner/repository changes remain audit metadata, not replay identity"
    );
    assert_eq!(reopened.rows().await.expect("rows").len(), 1);
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

#[tokio::test]
async fn scoped_organization_intent_survives_reopen_and_rename() {
    let scratch =
        crate::launch::harness::Scratch::new("discovery-org-rename").expect("scratch directory");
    let path = scratch.file();
    let journal = Journal::open(&path).await.expect("open journal");
    let mut store = JournalDiscoveryIntentStore::new(&journal);

    let registration = store
        .persist_scope_before(
            DiscoveryCredentialStep::OrganizationRegistrationToken,
            RegistrationScope::Organization { org: "Acme" },
            731,
            "Acme/RunnerRepo",
        )
        .await
        .expect("persist organization token intent");
    assert!(registration.get() > 0);
    assert_eq!(
        store
            .persist_scope_before(
                DiscoveryCredentialStep::OrganizationRegistrationToken,
                RegistrationScope::Organization { org: "acme" },
                731,
                "Acme/RenamedRepo",
            )
            .await,
        Err(SessionError::Uncertain),
        "same immutable target and scope cannot replay after rename"
    );
    assert_eq!(
        store
            .persist_scope_before(
                DiscoveryCredentialStep::OrganizationRegistrationToken,
                RegistrationScope::Enterprise {
                    enterprise: "AcmeEnterprise",
                },
                731,
                "Acme/RunnerRepo",
            )
            .await,
        Err(SessionError::Uncertain),
        "enterprise scope is explicitly unsupported before persistence"
    );
    assert_eq!(
        store
            .persist_before(
                DiscoveryCredentialStep::OrganizationRegistrationToken,
                731,
                "Acme/RunnerRepo",
            )
            .await,
        Err(SessionError::Uncertain),
        "organization issuance cannot fall back to a repository-only key"
    );
    assert_eq!(journal.rows().await.expect("rows").len(), 1);

    drop(store);
    drop(journal);
    let reopened = Journal::open(&path).await.expect("reopen journal");
    let mut reopened_store = JournalDiscoveryIntentStore::new(&reopened);
    assert_eq!(
        reopened_store
            .persist_scope_before(
                DiscoveryCredentialStep::OrganizationRegistrationToken,
                RegistrationScope::Organization { org: "ACME" },
                731,
                "Acme/RunnerRepo",
            )
            .await,
        Err(SessionError::Uncertain),
        "organization reservation survives process restart"
    );
}

#[tokio::test]
async fn scoped_discovery_keys_include_scope_kind_name_and_step() {
    let scratch =
        crate::launch::harness::Scratch::new("discovery-org-scope").expect("scratch directory");
    let journal = Journal::open(&scratch.file()).await.expect("open journal");
    let mut store = JournalDiscoveryIntentStore::new(&journal);

    let registration = store
        .persist_scope_before(
            DiscoveryCredentialStep::OrganizationRegistrationToken,
            RegistrationScope::Organization { org: "Acme" },
            731,
            "Acme/RunnerRepo",
        )
        .await
        .expect("persist organization token intent");
    let exchange = store
        .persist_scope_before(
            DiscoveryCredentialStep::ActionsAdminExchange,
            RegistrationScope::Organization { org: "Acme" },
            731,
            "Acme/RunnerRepo",
        )
        .await
        .expect("exchange is an independent operation");
    let other_scope = store
        .persist_scope_before(
            DiscoveryCredentialStep::OrganizationRegistrationToken,
            RegistrationScope::Organization { org: "OtherOrg" },
            731,
            "Acme/RunnerRepo",
        )
        .await
        .expect("different registration scope has a separate identity");
    let repository = store
        .persist_scope_before(
            DiscoveryCredentialStep::RepositoryRegistrationToken,
            RegistrationScope::Repository {
                owner: "Acme",
                repo: "RunnerRepo",
            },
            731,
            "Acme/RunnerRepo",
        )
        .await
        .expect("repository scope is distinct from organization scope");

    assert_ne!(registration.get(), exchange.get());
    assert_ne!(registration.get(), other_scope.get());
    assert_ne!(registration.get(), repository.get());
    assert_eq!(journal.rows().await.expect("rows").len(), 4);
}
