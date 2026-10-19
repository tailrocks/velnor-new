//! Connection isolation and shared reservation tests.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::error::HostError;
use crate::journal::{
    AssignmentIdentity, AssignmentKey, CapacityLimits, ConnectionJournal, IntentState, Journal,
    Outcome, ReservationRequest, ReservationResult, ReservationState, fixture_admission,
    fixture_profile,
};
use crate::launch::StartPermit;

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-journal-connection-{label}-{}-{id}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self { path })
    }

    fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }

    fn root(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.path);
    }
}

fn connection(id: &str, repository_id: i64, repository: &str) -> RepositoryConnection {
    let connection_id = ConnectionId::parse(id).expect("valid connection ID");
    let selector = ScaleSetSelector::try_new(
        "integration",
        &["velnor".to_owned(), "integration".to_owned()],
    )
    .expect("valid selector");
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        vec!["main".to_owned()],
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )
    .expect("valid policy");
    RepositoryConnection::try_new(
        connection_id,
        repository_id,
        repository.to_owned(),
        KeychainReference::parse(&format!("keychain:velnor/{id}")).expect("valid keychain ref"),
        selector,
        policy,
    )
    .expect("valid repository connection")
}

async fn scoped(
    journal: &Journal,
    id: &str,
    repository_id: i64,
    repository: &str,
) -> Result<ConnectionJournal, HostError> {
    journal.configure_storage_profile(fixture_profile()).await?;
    journal.bind_engine("test-engine").await?;
    let scope = journal
        .connection_scope(&connection(id, repository_id, repository))
        .await?;
    let scale_set_id = repository_id + 10_000;
    scope
        .bind_scale_set(scale_set_id, "integration", SetOwnership::Adopted)
        .await?;
    scope
        .bind_engine_context("test-engine", "unix:///var/run/docker.sock", "linux/amd64")
        .await?;
    Ok(scope)
}

fn limits(max_jobs: u32, cpu: u64, memory: u64) -> CapacityLimits {
    CapacityLimits {
        jobs: max_jobs,
        cpu_millicores: cpu,
        memory_bytes: memory,
    }
}

fn request<'a>(
    assignment: &'a AssignmentIdentity,
    sample_generation_id: &'a str,
) -> ReservationRequest<'a> {
    ReservationRequest {
        assignment,
        engine_id: "test-engine",
        docker_root_digest: docker_root_digest(),
        cpu_millicores: 2_000,
        memory_bytes: 8_589_934_592,
        sample_generation_id,
    }
}

async fn message_assignment(
    scope: &ConnectionJournal,
    message_id: i64,
) -> Result<AssignmentIdentity, HostError> {
    scope
        .assignment_identity(AssignmentKey::PopulationMessage { message_id })
        .await
}

fn docker_root_digest() -> &'static str {
    static DIGEST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DIGEST
        .get_or_init(|| {
            let mut hasher = Sha256::new();
            hasher.update(b"velnor-resource-probe-root-v1\0");
            hasher.update(b"/var/lib/docker");
            let intermediate = hasher.finalize();
            let digest = Sha256::digest(intermediate);
            let mut output = String::with_capacity(digest.len() * 2);
            for byte in digest {
                output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                output.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
            }
            output
        })
        .as_str()
}

fn permit() -> StartPermit {
    static NEXT_SAMPLE: AtomicU64 = AtomicU64::new(1);
    let generation = format!("{:032x}", NEXT_SAMPLE.fetch_add(1, Ordering::Relaxed));
    StartPermit::test_fixture_with_generation(&generation)
}

fn permit_with_generation(generation: &str) -> StartPermit {
    StartPermit::test_fixture_with_generation(generation)
}

async fn reserve(
    scope: &ConnectionJournal,
    assignment: &AssignmentIdentity,
) -> Result<ReservationResult, HostError> {
    let permit = permit();
    let generation = permit.sample_generation_id().to_owned();
    let storage = fixture_admission(&permit)?;
    scope
        .try_reserve_with_storage(request(assignment, &generation), permit, storage)
        .await
}

#[tokio::test]
async fn identical_remote_keys_are_isolated_by_connection() -> Result<(), HostError> {
    let scratch = Scratch::new("remote-collision")?;
    let journal = Journal::open(&scratch.file()).await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;

    let first_id = first.begin("session", "same-message").await?;
    let second_id = second.begin("session", "same-message").await?;
    assert_ne!(first_id, second_id);
    assert_eq!(first.begin("session", "same-message").await?, first_id);
    assert_eq!(second.begin("session", "same-message").await?, second_id);

    first.finish(first_id, Outcome::Done).await?;
    assert_eq!(first.read(first_id).await?, IntentState::Done);
    assert_eq!(second.read(second_id).await?, IntentState::Pending);
    assert_eq!(first.read(second_id).await, Err(HostError::Journal));
    drop(first);
    drop(second);
    drop(journal);

    let journal = Journal::open(&scratch.file()).await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    assert_eq!(first.read(first_id).await?, IntentState::Done);
    assert_eq!(second.read(second_id).await?, IntentState::Pending);
    assert_eq!(first.read(second_id).await, Err(HostError::Journal));
    Ok(())
}

#[tokio::test]
async fn version_one_rows_stay_unbound_and_block_new_connections() -> Result<(), HostError> {
    let scratch = Scratch::new("legacy-binding")?;
    let path = scratch.file();
    let text = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let sql_connection = database.connect().map_err(|_| HostError::Journal)?;
    sql_connection
        .execute_batch(include_str!("fixtures/version_one.sql"))
        .await
        .map_err(|_| HostError::Journal)?;
    drop(sql_connection);
    drop(database);

    let migrated = Journal::open(&path).await?;
    assert_eq!(migrated.read(1).await?, IntentState::Pending);
    assert_eq!(
        migrated
            .connection_scope(&connection("essential", 101, "donbeave/essential-mac",))
            .await
            .err(),
        Some(HostError::Journal)
    );
    assert_eq!(migrated.occupied_reservation_count().await?, 1);
    assert!(!migrated.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn connection_drain_and_retirement_do_not_cross_scopes() -> Result<(), HostError> {
    let scratch = Scratch::new("connection-drain")?;
    let journal = Journal::open(&scratch.file()).await?;
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let absent_binding = connection("offline", 303, "donbeave/offline-repo");
    assert!(
        !journal
            .retire_connection_if_drained(&absent_binding)
            .await?
    );
    assert!(!journal.is_retired_and_drained(&absent_binding).await?);
    let second_binding = connection("chainargos", 202, "ChainArgos/java-monorepo");
    assert!(!journal.is_retired_and_drained(&second_binding).await?);
    let second = journal.connection_scope(&second_binding).await?;
    second
        .bind_scale_set(10_202, "integration", SetOwnership::Adopted)
        .await?;
    second
        .bind_engine_context("test-engine", "unix:///var/run/docker.sock", "linux/amd64")
        .await?;
    let pending = first.begin("session", "remote-session").await?;

    assert!(!first.is_drained().await?);
    assert!(
        !journal
            .retire_connection_if_drained(&connection("essential", 101, "donbeave/essential-mac"))
            .await?
    );
    assert!(second.is_drained().await?);
    assert!(!journal.is_retired_and_drained(&second_binding).await?);
    assert!(second.retire_if_drained().await?);
    assert!(second.is_drained().await?);
    assert!(journal.is_retired_and_drained(&second_binding).await?);
    assert_eq!(
        journal
            .connection_scope(&connection("chainargos", 202, "ChainArgos/java-monorepo"))
            .await
            .err(),
        Some(HostError::Journal)
    );

    first.finish(pending, Outcome::Done).await?;
    first.record_cleanup(pending).await?;
    assert!(first.is_drained().await?);
    assert!(
        journal
            .retire_connection_if_drained(&connection("essential", 101, "donbeave/essential-mac"))
            .await?
    );
    assert!(first.retire_if_drained().await?);
    assert!(
        journal
            .is_retired_and_drained(&connection("essential", 101, "donbeave/essential-mac"))
            .await?
    );
    assert_eq!(journal.occupied_reservation_count().await?, 0);
    let exact = connection("chainargos", 202, "ChainArgos/java-monorepo");
    let changed = connection("chainargos", 202, "ChainArgos/other-repo");
    assert_eq!(
        journal.reactivate_if_drained(&changed).await,
        Err(HostError::Journal)
    );
    assert!(journal.reactivate_if_drained(&exact).await?);
    assert!(journal.connection_scope(&exact).await.is_ok());
    Ok(())
}

#[tokio::test]
async fn reactivating_drained_connection_is_not_blocked_by_active_sibling() -> Result<(), HostError>
{
    let scratch = Scratch::new("reactivate-sibling")?;
    let journal = Journal::open(&scratch.file()).await?;
    let first_binding = connection("essential", 101, "donbeave/essential-mac");
    let second_binding = connection("chainargos", 202, "ChainArgos/java-monorepo");
    let first = scoped(&journal, "essential", 101, "donbeave/essential-mac").await?;
    let second = scoped(&journal, "chainargos", 202, "ChainArgos/java-monorepo").await?;
    let sibling_work = first.begin("session", "active-session").await?;
    assert!(second.retire_if_drained().await?);
    assert!(journal.is_retired_and_drained(&second_binding).await?);
    assert!(!journal.is_retired_and_drained(&first_binding).await?);

    let changed = connection("chainargos", 202, "ChainArgos/other-repo");
    assert_eq!(
        journal.reactivate_if_drained(&changed).await,
        Err(HostError::Journal)
    );
    assert!(journal.reactivate_if_drained(&second_binding).await?);
    assert!(!first.is_drained().await?);
    assert!(second.is_drained().await?);
    assert_eq!(journal.occupied_reservation_count().await?, 0);

    first.finish(sibling_work, Outcome::Done).await?;
    first.record_cleanup(sibling_work).await?;
    Ok(())
}

#[path = "connection_lineage_tests.rs"]
mod lineage_tests;

#[path = "connection_tests/reservation.rs"]
mod reservation_tests;

#[path = "connection_tests/completion.rs"]
mod completion_tests;
