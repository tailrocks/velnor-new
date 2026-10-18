//! Durable image-import intent regressions.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::HostError;

use super::{
    ImageImportIdentity, ImageImportInspection, ImageImportObservation, ImageImportRequest,
    ImageImportState, Journal,
};

#[path = "identity_migration_tests.rs"]
mod identity_migration_tests;
#[path = "identity_tests.rs"]
mod identity_tests;

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new() -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-image-import-{}-{serial}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self { path })
    }

    fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.path);
    }
}

fn request(engine_id: &str) -> Result<ImageImportRequest, HostError> {
    ImageImportRequest::new(identity(engine_id))
}

fn identity(engine_id: &str) -> ImageImportIdentity {
    ImageImportIdentity {
        engine_id: engine_id.to_owned(),
        docker_root_digest: "b".repeat(64),
        daemon_architecture: "x86_64".to_owned(),
        source_sha: "a".repeat(40),
        archive_sha256: "c".repeat(64),
        archive_index_digest: "d".repeat(64),
        platform: "linux/amd64".to_owned(),
        manifest_digest: format!("sha256:{}", "e".repeat(64)),
        manifest_size: 128,
        config_digest: format!("sha256:{}", "f".repeat(64)),
        image_index_digest: None,
        image_index_size: None,
        load_archive_sha256: None,
        load_archive_index_digest: None,
        image_reference: "velnor-resource-probe:linux-amd64".to_owned(),
    }
}

async fn bound_journal(path: &std::path::Path) -> Result<Journal, HostError> {
    let journal = Journal::open(path).await?;
    journal.bind_engine("engine-a").await?;
    Ok(journal)
}

#[tokio::test]
async fn prepare_claim_and_verified_completion_are_idempotent() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = bound_journal(&scratch.file()).await?;
    let artifact = request("engine-a")?;

    let prepared = journal.prepare_image_import(artifact.clone()).await?;
    assert_eq!(prepared.state, ImageImportState::Prepared);
    assert_eq!(
        journal.prepare_image_import(artifact.clone()).await?,
        prepared
    );
    let first_journal = journal.clone();
    let second_journal = journal.clone();
    let (first_claim, second_claim) = tokio::join!(
        first_journal.claim_image_import_effect(prepared.id, &artifact),
        second_journal.claim_image_import_effect(prepared.id, &artifact),
    );
    let first_claim = first_claim?;
    let second_claim = second_claim?;
    assert_ne!(first_claim.is_some(), second_claim.is_some());
    let claim = first_claim.or(second_claim).ok_or(HostError::Journal)?;
    assert!(!journal.is_drained().await?);
    assert!(
        journal
            .claim_image_import_effect(prepared.id, &artifact)
            .await?
            .is_none()
    );

    let observation = classic_observation(&artifact);
    assert!(
        ImageImportInspection::verified(
            &artifact,
            "other-engine",
            artifact.docker_root_digest(),
            artifact.daemon_architecture(),
            observation.clone(),
        )
        .is_err()
    );
    assert!(
        ImageImportInspection::verified(
            &artifact,
            artifact.engine_id(),
            &"0".repeat(64),
            artifact.daemon_architecture(),
            observation.clone(),
        )
        .is_err()
    );
    let receipt = ImageImportInspection::verified(
        &artifact,
        artifact.engine_id(),
        artifact.docker_root_digest(),
        artifact.daemon_architecture(),
        observation,
    )?;
    journal
        .complete_claimed_image_import(claim, &receipt)
        .await?;
    let completed = journal.prepare_image_import(artifact.clone()).await?;
    assert_eq!(completed.id, prepared.id);
    assert_eq!(completed.state, ImageImportState::Completed);
    assert_eq!(completed.image_id, Some(receipt.image_id().to_owned()));
    assert!(completed.receipt.is_some());
    assert!(journal.is_drained().await?);

    let mut different_identity = identity("engine-a");
    different_identity.source_sha = "1".repeat(40);
    let different = ImageImportRequest::new(different_identity)?;
    assert_eq!(
        journal.prepare_image_import(different).await?.state,
        ImageImportState::Prepared
    );
    Ok(())
}

#[tokio::test]
async fn preloaded_completion_requires_an_unclaimed_prepared_row() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = bound_journal(&scratch.file()).await?;
    let request = request("engine-a")?;
    let prepared = journal.prepare_image_import(request.clone()).await?;
    let inspection = verified_inspection(&request)?;
    journal
        .complete_preloaded_image_import(prepared.id, &request, &inspection)
        .await?;
    assert!(journal.is_drained().await?);
    assert!(
        journal
            .claim_image_import_effect(prepared.id, &request)
            .await?
            .is_none()
    );
    assert!(
        journal
            .complete_preloaded_image_import(prepared.id, &request, &inspection)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn claimed_import_authority_is_bound_to_its_journal_file() -> Result<(), HostError> {
    let first_scratch = Scratch::new()?;
    let second_scratch = Scratch::new()?;
    let first_journal = bound_journal(&first_scratch.file()).await?;
    let second_journal = bound_journal(&second_scratch.file()).await?;
    let request = request("engine-a")?;
    let first = first_journal.prepare_image_import(request.clone()).await?;
    let second = second_journal.prepare_image_import(request.clone()).await?;
    let first_claim = first_journal
        .claim_image_import_effect(first.id, &request)
        .await?
        .ok_or(HostError::Journal)?;
    let second_claim = second_journal
        .claim_image_import_effect(second.id, &request)
        .await?
        .ok_or(HostError::Journal)?;

    let inspection = verified_inspection(&request)?;
    assert!(
        second_journal
            .complete_claimed_image_import(first_claim, &inspection)
            .await
            .is_err()
    );
    assert!(
        first_journal
            .complete_claimed_image_import(second_claim, &inspection)
            .await
            .is_err()
    );
    assert!(!first_journal.is_drained().await?);
    assert!(!second_journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn requested_and_uncertain_imports_survive_restart_without_reclaim() -> Result<(), HostError>
{
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = bound_journal(&path).await?;
    let request = request("engine-a")?;
    let requested = journal.prepare_image_import(request.clone()).await?;
    let claim = journal
        .claim_image_import_effect(requested.id, &request)
        .await?
        .ok_or(HostError::Journal)?;
    journal.mark_image_import_uncertain(claim).await?;
    assert!(!journal.is_drained().await?);
    drop(journal);

    let journal = bound_journal(&path).await?;
    let replay = journal.prepare_image_import(request.clone()).await?;
    assert_eq!(replay.id, requested.id);
    assert_eq!(replay.state, ImageImportState::Uncertain);
    let mut different_identity = identity("engine-a");
    different_identity.source_sha = "1".repeat(40);
    let different = ImageImportRequest::new(different_identity)?;
    assert_eq!(
        journal.prepare_image_import(different).await,
        Err(HostError::Journal)
    );
    assert!(
        journal
            .claim_image_import_effect(replay.id, &request)
            .await?
            .is_none()
    );
    assert!(
        journal
            .complete_preloaded_image_import(replay.id, &request, &verified_inspection(&request)?,)
            .await
            .is_err()
    );
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn requested_replay_cannot_complete_from_inspection_while_claim_is_lost()
-> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    let journal = bound_journal(&path).await?;
    let request = request("engine-a")?;
    let prepared = journal.prepare_image_import(request.clone()).await?;
    let claim = journal
        .claim_image_import_effect(prepared.id, &request)
        .await?
        .ok_or(HostError::Journal)?;
    drop(claim);
    drop(journal);

    let journal = bound_journal(&path).await?;
    let replay = journal.prepare_image_import(request.clone()).await?;
    assert_eq!(replay.state, ImageImportState::Requested);
    assert!(
        journal
            .claim_image_import_effect(replay.id, &request)
            .await?
            .is_none()
    );
    assert!(
        journal
            .complete_preloaded_image_import(replay.id, &request, &verified_inspection(&request)?,)
            .await
            .is_err()
    );
    assert!(!journal.is_drained().await?);
    Ok(())
}

#[tokio::test]
async fn only_pre_effect_abort_allows_a_new_attempt() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let journal = bound_journal(&scratch.file()).await?;
    let artifact = request("engine-a")?;
    let first = journal.prepare_image_import(artifact.clone()).await?;
    journal
        .abort_image_import_before_effect(first.id, &artifact)
        .await?;
    assert!(
        journal
            .claim_image_import_effect(first.id, &artifact)
            .await?
            .is_none()
    );
    let second = journal.prepare_image_import(artifact).await?;
    assert_ne!(first.id, second.id);
    assert_eq!(second.attempt, first.attempt + 1);
    assert_eq!(second.state, ImageImportState::Prepared);

    let wrong_engine = request("engine-b")?;
    assert_eq!(
        journal.prepare_image_import(wrong_engine).await,
        Err(HostError::Journal)
    );
    assert!(!journal.is_drained().await?);
    Ok(())
}

fn verified_inspection(request: &ImageImportRequest) -> Result<ImageImportInspection, HostError> {
    ImageImportInspection::verified(
        request,
        request.engine_id(),
        request.docker_root_digest(),
        request.daemon_architecture(),
        classic_observation(request),
    )
}

fn classic_observation(request: &ImageImportRequest) -> ImageImportObservation {
    ImageImportObservation::Classic {
        image_id: request.config_digest.clone(),
        repo_tags: vec![request.image_reference.clone()],
    }
}

#[tokio::test]
async fn version_four_migration_creates_and_validates_import_ledger() -> Result<(), HostError> {
    let scratch = Scratch::new()?;
    let path = scratch.file();
    drop(Journal::open(&path).await?);

    let text = path.to_str().ok_or(HostError::Journal)?;
    let database = turso::Builder::new_local(text)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let connection = database.connect().map_err(|_| HostError::Journal)?;
    for table in [
        "connection_runner_jits",
        "connection_request_authorizations",
        "connection_source_authorizations",
        "connection_scale_set_intents",
    ] {
        connection
            .execute(&format!("DROP TABLE {table}"), ())
            .await
            .map_err(|_| HostError::Journal)?;
    }
    connection
        .execute("DROP TABLE image_imports", ())
        .await
        .map_err(|_| HostError::Journal)?;
    connection
        .execute("PRAGMA user_version = 4", ())
        .await
        .map_err(|_| HostError::Journal)?;
    drop(connection);
    drop(database);

    let journal = Journal::open(&path).await?;
    let replay = journal.prepare_image_import(request("engine-a")?).await;
    assert_eq!(replay, Err(HostError::Journal));
    journal.bind_engine("engine-a").await?;
    assert_eq!(
        journal
            .prepare_image_import(request("engine-a")?)
            .await?
            .state,
        ImageImportState::Prepared
    );
    Ok(())
}
