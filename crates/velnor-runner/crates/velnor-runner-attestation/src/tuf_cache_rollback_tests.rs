use std::error::Error;
use std::fs;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use crate::tuf_state::{RootResponseCapture, TufCache, TufRefreshRequest};
use async_trait::async_trait;
use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::Ed25519KeyPair;
use bytes::Bytes;
use futures_util::StreamExt;
use jiff::{SignedDuration, Timestamp};
use tempfile::TempDir;
use tough::editor::RepositoryEditor;
use tough::editor::signed::{PathExists, SignedRole};
use tough::key_source::{KeySource, LocalKeySource};
use tough::schema::{KeyHolder, RoleType, Signed};
use tough::sign::Sign;
use tough::{FilesystemTransport, TargetName, Transport, TransportError};
use url::Url;

#[derive(Clone, Debug)]
struct FixtureTransport {
    inner: FilesystemTransport,
    capture: RootResponseCapture,
}

#[async_trait]
impl Transport for FixtureTransport {
    async fn fetch(&self, url: Url) -> Result<tough::TransportStream, TransportError> {
        let mut source = self.inner.fetch(url.clone()).await?;
        let mut body = Vec::new();
        while let Some(chunk) = source.next().await {
            body.extend_from_slice(&chunk?);
        }
        self.capture
            .record(&url, &body)
            .map_err(|_| TransportError::new(tough::TransportErrorKind::Other, "fixture"))?;
        Ok(Box::pin(futures_util::stream::iter(vec![Ok(Bytes::from(
            body,
        ))])))
    }
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tough")
}

fn directory_url(path: &Path) -> Url {
    Url::from_directory_path(path).expect("fixture path is absolute")
}

fn ignore_target(bytes: Option<&[u8]>) -> Result<(), Box<dyn Error>> {
    if bytes.is_some() {
        return Err("fixture unexpectedly requested a target".into());
    }
    Ok(())
}

async fn write_repository(
    root: &Path,
    key: &Path,
    target: &Path,
    destination: &Path,
    version: u64,
) -> Result<(), Box<dyn Error>> {
    let expiration = Timestamp::now() + SignedDuration::from_hours(72);
    let version = NonZeroU64::new(version).ok_or("repository version must be nonzero")?;
    let mut editor = RepositoryEditor::new(root).await?;
    editor
        .targets_version(version)?
        .targets_expires(expiration)?;
    editor
        .snapshot_version(version)
        .snapshot_expires(expiration);
    editor
        .timestamp_version(version)
        .timestamp_expires(expiration);
    editor.add_target_paths(vec![target.to_path_buf()]).await?;
    let keys: [Box<dyn KeySource>; 1] = [Box::new(LocalKeySource {
        path: key.to_path_buf(),
    })];
    let signed = editor.sign(&keys).await?;
    signed.write(destination.join("metadata")).await?;
    signed
        .link_targets(
            target.parent().ok_or("target has no parent")?,
            destination.join("targets"),
            PathExists::Skip,
        )
        .await?;
    Ok(())
}

async fn write_next_root(root: &Path, key: &Path, metadata: &Path) -> Result<(), Box<dyn Error>> {
    let root_bytes = fs::read(root)?;
    let original: Signed<tough::schema::Root> = serde_json::from_slice(&root_bytes)?;
    let mut next = original.signed.clone();
    next.version = NonZeroU64::new(2).ok_or("root version must be nonzero")?;
    let keys: [Box<dyn KeySource>; 1] = [Box::new(LocalKeySource {
        path: key.to_path_buf(),
    })];
    let signed = SignedRole::new(
        next,
        &KeyHolder::Root(original.signed.clone()),
        &keys,
        &SystemRandom::new(),
    )
    .await?;
    signed
        .write(metadata, original.signed.consistent_snapshot)
        .await?;
    Ok(())
}

fn capture_transport(capture: &RootResponseCapture) -> FixtureTransport {
    FixtureTransport {
        inner: FilesystemTransport,
        capture: capture.clone(),
    }
}

#[tokio::test]
async fn saved_root_stays_at_v2_when_same_key_signs_newer_timestamp() {
    let fixtures = fixtures();
    let root_path = fixtures.join("simple-rsa/root.json");
    let key_path = fixtures.join("snakeoil.pem");
    let target_path = fixtures.join("targets/file3.txt");
    let root1 = fs::read(&root_path).expect("read old bootstrap root");
    let temporary = TempDir::new().expect("create isolated test directory");
    let newer = temporary.path().join("newer");
    let stale_repository = temporary.path().join("stale");
    let cache_root = fs::canonicalize(temporary.path())
        .expect("canonicalize temporary cache parent")
        .join("cache");
    let cache = TufCache::new(&cache_root);

    write_repository(&root_path, &key_path, &target_path, &newer, 2)
        .await
        .expect("sign first timestamp");
    write_next_root(&root_path, &key_path, &newer.join("metadata"))
        .await
        .expect("write signed version-two root");
    assert!(newer.join("metadata/2.root.json").is_file());
    let capture = RootResponseCapture::new();
    let first = cache
        .refresh(TufRefreshRequest {
            bootstrap: &root1,
            migration: None,
            metadata_url: directory_url(&newer.join("metadata")),
            targets_url: directory_url(&newer.join("targets")),
            target_name: None::<&TargetName>,
            transport: capture_transport(&capture),
            capture: &capture,
            validate_target: ignore_target,
        })
        .await
        .expect("accept valid version-two root rotation");
    assert_eq!(first.root_chain.final_identity.version, 2);
    drop(first.repository);

    write_repository(&root_path, &key_path, &target_path, &stale_repository, 3)
        .await
        .expect("sign higher timestamp under the embedded old root");
    let capture = RootResponseCapture::new();
    let replay = cache
        .refresh(TufRefreshRequest {
            bootstrap: &root1,
            migration: None,
            metadata_url: directory_url(&stale_repository.join("metadata")),
            targets_url: directory_url(&stale_repository.join("targets")),
            target_name: None::<&TargetName>,
            transport: capture_transport(&capture),
            capture: &capture,
            validate_target: ignore_target,
        })
        .await
        .expect("continue from saved root rather than replaying embedded root");
    assert_eq!(replay.root_chain.identities[0].version, 2);
    assert_eq!(replay.root_chain.final_identity.version, 2);
    drop(replay.repository);

    let current = fs::read_to_string(cache_root.join("CURRENT")).expect("read CURRENT");
    let generation = cache_root.join("generations").join(current.trim());
    let high_water_state: serde_json::Value =
        serde_json::from_slice(&fs::read(generation.join("root-state.json")).expect("read state"))
            .expect("parse high-water state");
    let timestamp: serde_json::Value = serde_json::from_slice(
        &fs::read(generation.join("tuf/timestamp.json")).expect("read timestamp"),
    )
    .expect("parse timestamp metadata");
    let saved_root: serde_json::Value =
        serde_json::from_slice(&fs::read(generation.join("tuf/root.json")).expect("read root"))
            .expect("parse saved root");
    assert_eq!(high_water_state["root_version"], 2);
    assert_eq!(saved_root["signed"]["version"], 2);
    assert_eq!(timestamp["signed"]["version"], 3);
}

async fn write_repository_with_revoked_timestamp_key(
    root_path: &Path,
    old_key_path: &Path,
    target_path: &Path,
    destination: &Path,
) -> Result<(), Box<dyn Error>> {
    let root_bytes = fs::read(root_path)?;
    let original: Signed<tough::schema::Root> = serde_json::from_slice(&root_bytes)?;
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())?;
    let new_signer = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())?;
    let new_public = new_signer.tuf_key();
    let new_key_id = new_public.key_id()?;
    let mut next_root = original.signed.clone();
    next_root.version = NonZeroU64::new(2).ok_or("root version must be nonzero")?;
    next_root.keys.insert(new_key_id.clone(), new_public);
    for role in [RoleType::Timestamp, RoleType::Snapshot, RoleType::Targets] {
        next_root
            .roles
            .get_mut(&role)
            .ok_or("root role is missing")?
            .keyids = vec![new_key_id.clone()];
    }

    let old_keys: [Box<dyn KeySource>; 1] = [Box::new(LocalKeySource {
        path: old_key_path.to_path_buf(),
    })];
    let signed_root = SignedRole::new(
        next_root,
        &KeyHolder::Root(original.signed.clone()),
        &old_keys,
        &SystemRandom::new(),
    )
    .await?;
    let metadata_dir = destination.join("metadata");
    signed_root
        .write(&metadata_dir, original.signed.consistent_snapshot)
        .await?;
    let root2_path = metadata_dir.join("2.root.json");
    let rotated_root_bytes = fs::read(&root2_path)?;
    let root2: Signed<tough::schema::Root> = serde_json::from_slice(&rotated_root_bytes)?;
    assert!(
        !root2.signed.roles[&RoleType::Timestamp]
            .keyids
            .contains(&original.signed.roles[&RoleType::Timestamp].keyids[0])
    );

    fs::write(
        destination.join("rotated-timestamp-key.pk8"),
        pkcs8.as_ref(),
    )?;
    let expiration = Timestamp::now() + SignedDuration::from_hours(72);
    let version = NonZeroU64::new(2).ok_or("repository version must be nonzero")?;
    let mut editor = RepositoryEditor::new(&root2_path).await?;
    editor
        .targets_version(version)?
        .targets_expires(expiration)?;
    editor
        .snapshot_version(version)
        .snapshot_expires(expiration);
    editor
        .timestamp_version(version)
        .timestamp_expires(expiration);
    editor
        .add_target_paths(vec![target_path.to_path_buf()])
        .await?;
    let rotated_keys: [Box<dyn KeySource>; 1] = [Box::new(LocalKeySource {
        path: destination.join("rotated-timestamp-key.pk8"),
    })];
    let signed = editor.sign(&rotated_keys).await?;
    signed.write(&metadata_dir).await?;
    signed
        .link_targets(
            target_path.parent().ok_or("target has no parent")?,
            destination.join("targets"),
            PathExists::Skip,
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn revoked_v1_timestamp_key_cannot_sign_a_newer_timestamp_after_v2_rotation() {
    let fixtures = fixtures();
    let root_path = fixtures.join("simple-rsa/root.json");
    let old_key_path = fixtures.join("snakeoil.pem");
    let target_path = fixtures.join("targets/file3.txt");
    let root1 = fs::read(&root_path).expect("read old bootstrap root");
    let temporary = TempDir::new().expect("create isolated test directory");
    let newer = temporary.path().join("revoked-key-v2");
    let stale = temporary.path().join("stale-v1-key");
    let cache_root = fs::canonicalize(temporary.path())
        .expect("canonicalize temporary cache parent")
        .join("cache");
    let cache = TufCache::new(&cache_root);

    write_repository_with_revoked_timestamp_key(&root_path, &old_key_path, &target_path, &newer)
        .await
        .expect("create v2 repository that replaces timestamp/snapshot/targets keys");
    let capture = RootResponseCapture::new();
    let accepted = cache
        .refresh(TufRefreshRequest {
            bootstrap: &root1,
            migration: None,
            metadata_url: directory_url(&newer.join("metadata")),
            targets_url: directory_url(&newer.join("targets")),
            target_name: None::<&TargetName>,
            transport: capture_transport(&capture),
            capture: &capture,
            validate_target: ignore_target,
        })
        .await
        .expect("accept signed root v2 and its replacement role key");
    assert_eq!(accepted.root_chain.final_identity.version, 2);
    drop(accepted.repository);

    write_repository(&root_path, &old_key_path, &target_path, &stale, 3)
        .await
        .expect("sign a newer timestamp using the revoked v1 role key");
    let pointer_before = fs::read_to_string(cache_root.join("CURRENT"))
        .expect("read active generation before rejected replay");
    let capture = RootResponseCapture::new();
    let rejected = cache
        .refresh(TufRefreshRequest {
            bootstrap: &root1,
            migration: None,
            metadata_url: directory_url(&stale.join("metadata")),
            targets_url: directory_url(&stale.join("targets")),
            target_name: None::<&TargetName>,
            transport: capture_transport(&capture),
            capture: &capture,
            validate_target: ignore_target,
        })
        .await;
    assert!(
        rejected.is_err(),
        "version-three timestamp signed with revoked key must be rejected"
    );
    assert_eq!(
        fs::read_to_string(cache_root.join("CURRENT"))
            .expect("active generation remains after rejection"),
        pointer_before
    );
    let active_id = fs::read_to_string(cache_root.join("CURRENT"))
        .expect("read active pointer")
        .trim()
        .to_owned();
    let active_state: serde_json::Value = serde_json::from_slice(
        &fs::read(
            cache_root
                .join("generations")
                .join(active_id)
                .join("root-state.json"),
        )
        .expect("read active high-water state"),
    )
    .expect("parse active high-water state");
    assert_eq!(active_state["root_version"], 2);
}
