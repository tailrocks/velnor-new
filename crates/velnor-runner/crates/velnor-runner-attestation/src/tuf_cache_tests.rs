use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use crate::root_chain::RootIdentity;
use crate::tuf_state::{BootstrapMigration, RootResponseCapture, TufCache, TufRefreshRequest};
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use tempfile::TempDir;
use tough::{
    FilesystemTransport, TargetName, Transport, TransportError, TransportErrorKind, TransportStream,
};
use url::Url;

#[derive(Clone, Debug)]
struct CapturingFilesystemTransport {
    inner: FilesystemTransport,
    capture: RootResponseCapture,
}

#[async_trait]
impl Transport for CapturingFilesystemTransport {
    async fn fetch(&self, url: Url) -> Result<TransportStream, TransportError> {
        let mut source = self.inner.fetch(url.clone()).await?;
        let mut body = Vec::new();
        while let Some(chunk) = source.next().await {
            let chunk = chunk?;
            body.extend_from_slice(&chunk);
        }
        self.capture
            .record(&url, &body)
            .map_err(|_| TransportError::new(TransportErrorKind::Other, "fixture transport"))?;
        Ok(Box::pin(futures_util::stream::iter(vec![Ok(Bytes::from(
            body,
        ))])))
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/tough")
        .join(name)
}

fn private_cache_root(temporary: &TempDir) -> PathBuf {
    fs::canonicalize(temporary.path())
        .expect("canonicalize temporary cache parent")
        .join("cache")
}

fn directory_url(path: &Path) -> Url {
    Url::from_directory_path(path).expect("fixture directory path is absolute")
}

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = Sha256::digest(bytes);
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(HEX[usize::from(byte >> 4)] as char);
        value.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    value
}

fn ignore_target(bytes: Option<&[u8]>) -> Result<(), Box<dyn Error>> {
    if bytes.is_some() {
        return Err("fixture unexpectedly requested a target".into());
    }
    Ok(())
}

fn transport(capture: RootResponseCapture) -> CapturingFilesystemTransport {
    CapturingFilesystemTransport {
        inner: FilesystemTransport,
        capture,
    }
}

async fn refresh(
    cache: &TufCache,
    bootstrap: &[u8],
    migration: Option<&BootstrapMigration>,
    repository: &Path,
    capture: &RootResponseCapture,
) -> Result<Vec<RootIdentity>, Box<dyn Error>> {
    let result = cache
        .refresh(TufRefreshRequest {
            bootstrap,
            migration,
            metadata_url: directory_url(repository),
            targets_url: directory_url(repository),
            target_name: None::<&TargetName>,
            transport: transport(capture.clone()),
            capture,
            validate_target: ignore_target,
        })
        .await?;
    let identities = result.root_chain.identities;
    drop(result.repository);
    Ok(identities)
}

async fn initial_rotated_refresh(
    cache: &TufCache,
    capture: &RootResponseCapture,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn Error>> {
    let repository = fixture("rotated-root");
    let root1 = fs::read(repository.join("1.root.json"))?;
    let root2 = fs::read(repository.join("2.root.json"))?;
    refresh(cache, &root1, None, &repository, capture).await?;
    Ok((root1, root2))
}

fn current_generation(cache_root: &Path) -> String {
    fs::read_to_string(cache_root.join("CURRENT"))
        .expect("read active generation pointer")
        .trim()
        .to_owned()
}

fn high_water(cache_root: &Path) -> serde_json::Value {
    let path = cache_root
        .join("generations")
        .join(current_generation(cache_root))
        .join("root-state.json");
    serde_json::from_slice(&fs::read(path).expect("read root state"))
        .expect("parse root state JSON")
}

#[tokio::test]
async fn fresh_rotation_then_saved_root_no_rotation_succeeds() {
    let temporary = TempDir::new().expect("create isolated cache root");
    let cache_root = private_cache_root(&temporary);
    let cache = TufCache::new(&cache_root);
    let capture = RootResponseCapture::new();
    let (root1, _root2) = initial_rotated_refresh(&cache, &capture)
        .await
        .expect("Tough validates the signed root rotation");

    let state = high_water(&cache_root);
    assert_eq!(state["root_version"], 2);
    assert_eq!(state["bootstrap_sha256"], digest(&root1));

    let chain = refresh(
        &cache,
        &root1,
        None,
        &fixture("rotated-root"),
        &RootResponseCapture::new(),
    )
    .await
    .expect("saved version-two root with no fetched rotation remains valid");
    assert_eq!(chain.len(), 1);
    assert_eq!(chain[0].version, 2);
    assert_eq!(high_water(&cache_root)["root_version"], 2);
}

#[tokio::test]
async fn bootstrap_migration_requires_exact_record_and_authenticated_chain_membership() {
    let temporary = TempDir::new().expect("create isolated cache root");
    let cache_root = private_cache_root(&temporary);
    let cache = TufCache::new(&cache_root);
    let capture = RootResponseCapture::new();
    let (root1, root2) = initial_rotated_refresh(&cache, &capture)
        .await
        .expect("initial signed rotation");
    let root1_identity = RootIdentity::from_bytes(&root1).expect("canonical prior root");
    let root2_identity = RootIdentity::from_bytes(&root2).expect("canonical new root");
    let migration = BootstrapMigration {
        from_sha256: digest(&root1),
        from_root: root1_identity.clone(),
        to_sha256: digest(&root2),
        to_root: root2_identity,
    };

    let pointer_before = current_generation(&cache_root);
    let mut wrong = migration.clone();
    wrong.to_sha256 = "0".repeat(64);
    let error = refresh(
        &cache,
        &root2,
        Some(&wrong),
        &fixture("rotated-root"),
        &RootResponseCapture::new(),
    )
    .await
    .expect_err("migration with a false bootstrap digest must fail closed");
    assert!(
        error
            .to_string()
            .contains("bootstrap migration identity mismatch"),
        "unexpected migration error: {error:?}"
    );
    assert_eq!(current_generation(&cache_root), pointer_before);

    let chain = refresh(
        &cache,
        &root2,
        Some(&migration),
        &fixture("rotated-root"),
        &RootResponseCapture::new(),
    )
    .await
    .expect("new bootstrap exactly matches authenticated prior rotation");
    assert_eq!(chain.len(), 1);
    assert_eq!(
        chain[0],
        RootIdentity::from_bytes(&root2).expect("canonical new root")
    );
    assert_eq!(high_water(&cache_root)["bootstrap_sha256"], digest(&root2));
}

#[tokio::test]
async fn duplicate_captured_root_response_never_publishes_generation() {
    let temporary = TempDir::new().expect("create isolated cache root");
    let cache_root = private_cache_root(&temporary);
    let cache = TufCache::new(&cache_root);
    let repository = fixture("rotated-root");
    let root1 = fs::read(repository.join("1.root.json")).expect("read initial root");
    let root2 = fs::read(repository.join("2.root.json")).expect("read next root");
    let capture = RootResponseCapture::new();
    capture
        .record(
            &Url::parse("https://fixture.invalid/2.root.json").expect("root URL"),
            &root2,
        )
        .expect("seed duplicate root response");

    let error = refresh(&cache, &root1, None, &repository, &capture)
        .await
        .expect_err("duplicate root version must be rejected");
    assert!(
        error
            .to_string()
            .contains("duplicate root version response")
    );
    assert!(!cache_root.join("CURRENT").exists());
}

async fn cache_after_initial_rotation() -> (TempDir, PathBuf, TufCache, Vec<u8>) {
    let temporary = TempDir::new().expect("create initialized cache");
    let cache_root = private_cache_root(&temporary);
    let cache = TufCache::new(&cache_root);
    let capture = RootResponseCapture::new();
    let (root1, _) = initial_rotated_refresh(&cache, &capture)
        .await
        .expect("initial signed rotation");
    (temporary, cache_root, cache, root1)
}

#[tokio::test]
async fn corrupt_current_is_not_reinitialized() {
    let (_temporary, cache_root, cache, root1) = cache_after_initial_rotation().await;
    let repository = fixture("rotated-root");
    fs::write(cache_root.join("CURRENT"), b"../../outside\n").expect("corrupt the active pointer");
    assert!(
        refresh(
            &cache,
            &root1,
            None,
            &repository,
            &RootResponseCapture::new()
        )
        .await
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(cache_root.join("CURRENT")).expect("pointer remains corrupt"),
        "../../outside\n"
    );
}

#[tokio::test]
async fn missing_current_is_not_reinitialized() {
    let (_temporary, cache_root, cache, root1) = cache_after_initial_rotation().await;
    let repository = fixture("rotated-root");
    fs::remove_file(cache_root.join("CURRENT")).expect("remove current pointer");
    assert!(
        refresh(
            &cache,
            &root1,
            None,
            &repository,
            &RootResponseCapture::new()
        )
        .await
        .is_err()
    );
    assert!(!cache_root.join("CURRENT").exists());
}

#[tokio::test]
async fn corrupt_root_high_water_is_not_reinitialized() {
    let (_temporary, cache_root, cache, root1) = cache_after_initial_rotation().await;
    let repository = fixture("rotated-root");
    let generation = current_generation(&cache_root);
    let state = cache_root
        .join("generations")
        .join(&generation)
        .join("root-state.json");
    fs::write(&state, b"{}").expect("corrupt high-water state");
    assert!(
        refresh(
            &cache,
            &root1,
            None,
            &repository,
            &RootResponseCapture::new()
        )
        .await
        .is_err()
    );
    assert_eq!(current_generation(&cache_root), generation);
    assert_eq!(fs::read(state).expect("corrupt state remains"), b"{}");
}

#[tokio::test]
async fn missing_root_high_water_is_not_reinitialized() {
    let (_temporary, cache_root, cache, root1) = cache_after_initial_rotation().await;
    let repository = fixture("rotated-root");
    let generation = current_generation(&cache_root);
    let state = cache_root
        .join("generations")
        .join(&generation)
        .join("root-state.json");
    fs::remove_file(&state).expect("remove high-water state");
    assert!(
        refresh(
            &cache,
            &root1,
            None,
            &repository,
            &RootResponseCapture::new()
        )
        .await
        .is_err()
    );
    assert_eq!(current_generation(&cache_root), generation);
    assert!(!state.exists());
}
