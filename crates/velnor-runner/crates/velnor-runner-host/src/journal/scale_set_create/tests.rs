//! Scale Set create intent regressions.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{
    ConnectionId, KeychainReference, RepositoryConnection, ScaleSetSelector, SourcePolicy,
    SourcePolicyMode, WorkflowEvent,
};
use crate::connect::SetOwnership;
use crate::error::HostError;

use super::{
    CreatedScaleSetResponse, ExistingScaleSetObservation, Journal, ScaleSetCreateNoEffect,
    ScaleSetCreateRequest, ScaleSetCreateState,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-scale-set-create-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self(path))
    }

    fn file(&self) -> PathBuf {
        self.0.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn binding() -> Result<RepositoryConnection, HostError> {
    let connection_id = ConnectionId::parse("essential")?;
    let labels = [
        "velnor".to_owned(),
        "essential".to_owned(),
        "linux".to_owned(),
    ];
    let selector = ScaleSetSelector::try_new("essential", &labels)?;
    let policy = SourcePolicy::try_new(
        SourcePolicyMode::BranchRestricted,
        vec!["main".to_owned()],
        Vec::new(),
        vec![WorkflowEvent::Push],
        Vec::new(),
    )?;
    RepositoryConnection::try_new(
        connection_id,
        101,
        "donbeave/essential-mac".to_owned(),
        KeychainReference::parse("keychain:velnor/essential")?,
        selector,
        policy,
    )
}

async fn open_scope(
    path: &std::path::Path,
    binding: &RepositoryConnection,
) -> Result<(Journal, crate::journal::connection::ConnectionJournal), HostError> {
    let journal = Journal::open(path).await?;
    journal.bind_engine("test-engine").await?;
    let scope = journal.connection_scope(binding).await?;
    Ok((journal, scope))
}

fn request(binding: &RepositoryConnection) -> Result<ScaleSetCreateRequest, HostError> {
    ScaleSetCreateRequest::new(binding, 7)
}

fn labels() -> Vec<String> {
    vec![
        "velnor".to_owned(),
        "essential".to_owned(),
        "linux".to_owned(),
    ]
}

fn created_response(request: &ScaleSetCreateRequest) -> Result<CreatedScaleSetResponse, HostError> {
    CreatedScaleSetResponse::verified(request, 91, "essential", &labels(), true)
}

fn existing_observation(
    request: &ScaleSetCreateRequest,
) -> Result<ExistingScaleSetObservation, HostError> {
    ExistingScaleSetObservation::verified(request, 91, "essential", &labels(), true)
}

#[tokio::test]
async fn prepare_replay_claim_and_create_receipt_survive_restart() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let binding = binding()?;
    let (journal, scope) = open_scope(&path, &binding).await?;
    let request = request(&binding)?;
    let prepared = scope.prepare_scale_set_create(&request).await?;
    assert_eq!(prepared.state(), ScaleSetCreateState::Prepared);
    assert_eq!(scope.prepare_scale_set_create(&request).await?, prepared);
    assert!(!scope.is_drained().await?);

    let first = scope.claim_scale_set_create(&prepared).await?;
    assert!(first.is_some());
    assert!(scope.claim_scale_set_create(&prepared).await?.is_none());
    let created = scope
        .record_scale_set_created(
            first.ok_or(HostError::Journal)?,
            created_response(&request)?,
        )
        .await?;
    assert_eq!(created.state(), ScaleSetCreateState::Created);
    let receipt = created.receipt().ok_or(HostError::Journal)?;
    assert_eq!(receipt.id, 91);
    assert_eq!(receipt.name, "essential");
    assert_eq!(receipt.labels, labels());
    assert!(receipt.disable_update);
    assert_eq!(receipt.ownership, SetOwnership::Created);
    assert!(scope.is_drained().await?);
    assert_eq!(scope.scale_set().await?.ok_or(HostError::Journal)?.id, 91);
    drop(scope);
    drop(journal);

    let (_journal, scope) = open_scope(&path, &binding).await?;
    let replay = scope.prepare_scale_set_create(&request).await?;
    assert_eq!(replay, created);
    assert!(scope.claim_scale_set_create(&replay).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn uncertain_create_stays_charged_and_cannot_be_adopted_after_restart()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let binding = binding()?;
    let (journal, scope) = open_scope(&path, &binding).await?;
    let request = request(&binding)?;
    let prepared = scope.prepare_scale_set_create(&request).await?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await?
        .ok_or(HostError::Journal)?;
    let uncertain = scope.mark_scale_set_create_uncertain(claim).await?;
    assert_eq!(uncertain.state(), ScaleSetCreateState::Uncertain);
    assert!(!journal.is_drained().await?);
    drop(scope);
    drop(journal);

    let (journal, scope) = open_scope(&path, &binding).await?;
    let replay = scope.prepare_scale_set_create(&request).await?;
    assert_eq!(replay.state(), ScaleSetCreateState::Uncertain);
    assert!(scope.claim_scale_set_create(&replay).await?.is_none());
    assert!(
        scope
            .adopt_scale_set(&request, existing_observation(&request)?)
            .await
            .is_err()
    );
    assert!(!scope.is_drained().await?);
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn only_conflict_no_effect_can_be_followed_by_exact_adoption() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let binding = binding()?;
    let (journal, scope) = open_scope(&scratch.file(), &binding).await?;
    let request = request(&binding)?;
    assert!(ScaleSetCreateNoEffect::from_http_status(404).is_err());
    let prepared = scope.prepare_scale_set_create(&request).await?;
    let claim = scope
        .claim_scale_set_create(&prepared)
        .await?
        .ok_or(HostError::Journal)?;
    let no_effect = scope
        .record_scale_set_create_no_effect(claim, ScaleSetCreateNoEffect::from_http_status(409)?)
        .await?;
    assert_eq!(no_effect.state(), ScaleSetCreateState::NoEffect);
    assert_eq!(no_effect.no_effect_status(), Some(409));
    assert!(scope.claim_scale_set_create(&no_effect).await?.is_none());

    let adopted = scope
        .adopt_scale_set(&request, existing_observation(&request)?)
        .await?;
    assert_eq!(adopted.state(), ScaleSetCreateState::Adopted);
    assert_eq!(adopted.no_effect_status(), Some(409));
    assert_eq!(
        adopted.receipt().ok_or(HostError::Journal)?.ownership,
        SetOwnership::Adopted
    );
    assert!(scope.is_drained().await?);
    assert!(journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn existing_set_adoption_binds_exact_fields_without_create_claim() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let binding = binding()?;
    let (_journal, scope) = open_scope(&scratch.file(), &binding).await?;
    let request = request(&binding)?;
    let wrong_name = ExistingScaleSetObservation::verified(&request, 91, "other", &labels(), true);
    let wrong_labels = ExistingScaleSetObservation::verified(
        &request,
        91,
        "essential",
        &["velnor".to_owned(), "essential".to_owned()],
        true,
    );
    let updates_enabled =
        ExistingScaleSetObservation::verified(&request, 91, "essential", &labels(), false);
    assert!(wrong_name.is_err());
    assert!(wrong_labels.is_err());
    assert!(updates_enabled.is_err());

    let adopted = scope
        .adopt_scale_set(&request, existing_observation(&request)?)
        .await?;
    assert_eq!(adopted.state(), ScaleSetCreateState::Adopted);
    assert!(scope.claim_scale_set_create(&adopted).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn version_five_migration_adds_scale_set_intent_table() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    drop(Journal::open(&path).await?);
    let (database, connection) = raw_database(&path).await?;
    connection
        .execute("DROP TABLE connection_scale_set_intents", ())
        .await
        .map_err(|_| HostError::Journal)?;
    reset_image_imports_to_version_five(&connection).await?;
    connection
        .execute("PRAGMA user_version = 5", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let _journal = Journal::open(&path).await?;
    assert_eq!(journal_version(&path).await?, 7);
    let (_, connection) = raw_database(&path).await?;
    let mut rows = connection
        .query("PRAGMA table_info(connection_scale_set_intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    assert!(rows.next().await.map_err(|_| HostError::Journal)?.is_some());
    Ok(())
}

#[tokio::test]
async fn malformed_version_five_scale_set_table_fails_without_mutation() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    drop(Journal::open(&path).await?);
    let (database, connection) = raw_database(&path).await?;
    connection
        .execute("DROP TABLE connection_scale_set_intents", ())
        .await
        .map_err(|_| HostError::Journal)?;
    reset_image_imports_to_version_five(&connection).await?;
    connection
        .execute("CREATE TABLE connection_scale_set_intents (wrong TEXT)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 5", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    assert!(Journal::open(&path).await.is_err());
    assert_eq!(journal_version(&path).await?, 5);
    let (_, connection) = raw_database(&path).await?;
    let mut rows = connection
        .query("PRAGMA table_info(connection_scale_set_intents)", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let column = rows
        .next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?;
    assert_eq!(
        column.get::<String>(1).map_err(|_| HostError::Journal)?,
        "wrong"
    );
    Ok(())
}

async fn raw_database(
    path: &std::path::Path,
) -> Result<(turso::Database, turso::Connection), HostError> {
    let text = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    Ok((database, connection))
}

async fn journal_version(path: &std::path::Path) -> Result<i64, HostError> {
    let (_database, connection) = raw_database(path).await?;
    let mut rows = connection
        .query("PRAGMA user_version", ())
        .await
        .map_err(|_| HostError::Journal)?;
    rows.next()
        .await
        .map_err(|_| HostError::Journal)?
        .ok_or(HostError::Journal)?
        .get(0)
        .map_err(|_| HostError::Journal)
}

async fn reset_image_imports_to_version_five(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    connection
        .execute("DROP TABLE image_imports", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE TABLE image_imports (id INTEGER PRIMARY KEY AUTOINCREMENT, engine_id TEXT NOT NULL, docker_root_digest TEXT NOT NULL CHECK (length(docker_root_digest) = 64 AND docker_root_digest NOT GLOB '*[^0-9a-f]*'), daemon_architecture TEXT NOT NULL CHECK (daemon_architecture IN ('x86_64', 'aarch64')), source_sha TEXT NOT NULL CHECK (length(source_sha) = 40 AND source_sha NOT GLOB '*[^0-9a-f]*'), archive_sha256 TEXT NOT NULL CHECK (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9a-f]*'), index_digest TEXT NOT NULL CHECK (length(index_digest) = 64 AND index_digest NOT GLOB '*[^0-9a-f]*'), platform TEXT NOT NULL CHECK (platform = 'linux/amd64'), manifest_digest TEXT NOT NULL CHECK (length(manifest_digest) = 71 AND substr(manifest_digest, 1, 7) = 'sha256:' AND substr(manifest_digest, 8) NOT GLOB '*[^0-9a-f]*'), config_digest TEXT NOT NULL CHECK (length(config_digest) = 71 AND substr(config_digest, 1, 7) = 'sha256:' AND substr(config_digest, 8) NOT GLOB '*[^0-9a-f]*'), attempt INTEGER NOT NULL CHECK (attempt >= 0), state TEXT NOT NULL CHECK (state IN ('prepared', 'requested', 'uncertain', 'completed', 'aborted')), image_id TEXT, CHECK ((state = 'completed' AND image_id IS NOT NULL) OR (state != 'completed' AND image_id IS NULL)), UNIQUE (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt))",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute(
            "CREATE INDEX image_imports_engine_state ON image_imports(engine_id, state, id)",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    Ok(())
}
