use std::fs;
use std::num::NonZeroU64;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use aws_lc_rs::rand::SystemRandom;
use jiff::{SignedDuration, Timestamp};
use tempfile::TempDir;
use tough::editor::RepositoryEditor;
use tough::editor::signed::{PathExists, SignedRole};
use tough::error::Error as ToughError;
use tough::key_source::{KeySource, LocalKeySource};
use tough::schema::{KeyHolder, RoleType};
use tough::{ExpirationEnforcement, RepositoryLoader};
use url::Url;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tough")
        .join(name)
}

fn directory_url(path: &Path) -> Url {
    Url::from_directory_path(path).expect("fixture path is absolute")
}

async fn load_from_directories(
    root: &Vec<u8>,
    metadata: &Path,
    targets: &Path,
    datastore: Option<&Path>,
) -> Result<tough::Repository, Box<dyn std::error::Error>> {
    let mut loader = RepositoryLoader::new(root, directory_url(metadata), directory_url(targets))
        .expiration_enforcement(ExpirationEnforcement::Safe);
    if let Some(datastore) = datastore {
        loader = loader.datastore(datastore);
    }
    Ok(loader.load().await?)
}

#[tokio::test]
async fn authentic_signed_expired_timestamp_is_rejected() {
    let metadata = fixture("expired-repository/metadata");
    let root = fs::read(metadata.join("1.root.json")).expect("read official fixture root");
    let result = load_from_directories(&root, &metadata, &metadata, None).await;
    let error = result.expect_err("Safe expiration must reject the fixture");
    let tough_error = error
        .downcast_ref::<ToughError>()
        .expect("expiration failure should retain the Tough error type");
    assert!(matches!(
        tough_error,
        ToughError::ExpiredMetadata {
            role: RoleType::Timestamp,
            ..
        }
    ));
}

#[tokio::test]
async fn official_root_rotation_fixture_accepts_version_two() {
    let metadata = fixture("rotated-root");
    let root = fs::read(metadata.join("1.root.json")).expect("read official fixture root");
    let repository = load_from_directories(&root, &metadata, &metadata, None)
        .await
        .expect("valid old-and-new signed root rotation");
    assert_eq!(u64::from(repository.root().signed.version), 2);
}

#[tokio::test]
async fn tampered_root_rotation_signature_is_rejected() {
    let source = fixture("rotated-root");
    let temporary = TempDir::new().expect("create isolated repository copy");
    for name in [
        "1.root.json",
        "2.root.json",
        "1.targets.json",
        "1.snapshot.json",
        "timestamp.json",
    ] {
        fs::copy(source.join(name), temporary.path().join(name))
            .expect("copy official rotation fixture");
    }
    let next_root_path = temporary.path().join("2.root.json");
    let mut permissions = fs::metadata(&next_root_path)
        .expect("read copied fixture permissions")
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&next_root_path, permissions).expect("make isolated copy writable");
    let root_bytes = fs::read(&next_root_path).expect("read official next-root fixture");
    let mut next_root: serde_json::Value =
        serde_json::from_slice(&root_bytes).expect("parse official rotation fixture");
    next_root["signatures"][0]["sig"] = serde_json::Value::String("00".to_owned());
    fs::write(
        next_root_path,
        serde_json::to_vec(&next_root).expect("serialize modified fixture"),
    )
    .expect("write modified rotation fixture");

    let root = fs::read(temporary.path().join("1.root.json")).expect("read bootstrap root");
    let result = load_from_directories(&root, temporary.path(), temporary.path(), None).await;
    let error = result.expect_err("invalid root rotation signature must fail closed");
    let tough_error = error
        .downcast_ref::<ToughError>()
        .expect("rotation failure should retain the Tough error type");
    assert!(matches!(
        tough_error,
        ToughError::VerifyMetadata {
            role: RoleType::Root,
            ..
        }
    ));
}

#[tokio::test]
async fn expired_bootstrap_root_without_rotation_is_rejected() {
    let source = fixture("rotated-root");
    let temporary = TempDir::new().expect("create isolated repository copy");
    for name in [
        "1.root.json",
        "1.targets.json",
        "1.snapshot.json",
        "timestamp.json",
    ] {
        fs::copy(source.join(name), temporary.path().join(name))
            .expect("copy old-root fixture without the next root");
    }
    let root = fs::read(temporary.path().join("1.root.json")).expect("read expired bootstrap root");
    let result = load_from_directories(&root, temporary.path(), temporary.path(), None).await;
    let error = result.expect_err("missing rotation must not leave an expired root trusted");
    let tough_error = error
        .downcast_ref::<ToughError>()
        .expect("expired-root failure should retain the Tough error type");
    assert!(matches!(
        tough_error,
        ToughError::ExpiredMetadata {
            role: RoleType::Root,
            ..
        }
    ));
}

async fn write_signed_repository(
    root: &Path,
    key: &Path,
    target: &Path,
    destination: &Path,
    version: u64,
) -> Result<(), Box<dyn std::error::Error>> {
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
    let metadata = destination.join("metadata");
    let targets = destination.join("targets");
    signed.write(&metadata).await?;
    signed
        .link_targets(
            target.parent().ok_or("target has no parent")?,
            &targets,
            PathExists::Skip,
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn persisted_datastore_rejects_older_signed_timestamp() {
    let fixtures = fixture("");
    let root_path = fixtures.join("simple-rsa/root.json");
    let key_path = fixtures.join("snakeoil.pem");
    let target_path = fixtures.join("targets/file3.txt");
    let root = fs::read(&root_path).expect("read test signing root");
    let temporary = TempDir::new().expect("create temporary repository");
    let newer = temporary.path().join("newer");
    let older = temporary.path().join("older");
    let datastore = temporary.path().join("persistent-datastore");
    fs::create_dir(&datastore).expect("create persistent datastore");
    write_signed_repository(&root_path, &key_path, &target_path, &newer, 2)
        .await
        .expect("sign newer fixture using Tough test key");
    write_signed_repository(&root_path, &key_path, &target_path, &older, 1)
        .await
        .expect("sign older fixture using Tough test key");

    load_from_directories(
        &root,
        &newer.join("metadata"),
        &newer.join("targets"),
        Some(&datastore),
    )
    .await
    .expect("load newer signed repository");
    let result = load_from_directories(
        &root,
        &older.join("metadata"),
        &older.join("targets"),
        Some(&datastore),
    )
    .await;
    let error = result.expect_err("persisted metadata must reject rollback");
    let tough_error = error
        .downcast_ref::<ToughError>()
        .expect("rollback failure should retain the Tough error type");
    assert!(matches!(
        tough_error,
        ToughError::OlderMetadata {
            role: RoleType::Timestamp,
            current_version: 2,
            new_version: 1,
            ..
        }
    ));
}

#[tokio::test]
async fn loader_can_replay_unexpired_old_root_despite_persisted_newer_root() {
    let fixtures = fixture("");
    let root_path = fixtures.join("simple-rsa/root.json");
    let key_path = fixtures.join("snakeoil.pem");
    let target_path = fixtures.join("targets/file3.txt");
    let root_bytes = fs::read(&root_path).expect("read test signing root");
    let original_root: tough::schema::Signed<tough::schema::Root> =
        serde_json::from_slice(&root_bytes).expect("parse test signing root");
    let mut next_root = original_root.signed.clone();
    next_root.version = NonZeroU64::new(2).expect("root version is nonzero");
    let keys: [Box<dyn KeySource>; 1] = [Box::new(LocalKeySource { path: key_path })];
    let next_root = SignedRole::new(
        next_root,
        &KeyHolder::Root(original_root.signed.clone()),
        &keys,
        &SystemRandom::new(),
    )
    .await
    .expect("sign sequential root using Tough's test key interface");

    let temporary = TempDir::new().expect("create temporary repository");
    let newer_root_repo = temporary.path().join("newer-root");
    let stale_root_repo = temporary.path().join("stale-root");
    let datastore = temporary.path().join("persistent-datastore");
    fs::create_dir(&datastore).expect("create persistent datastore");
    write_signed_repository(
        &root_path,
        &fixtures.join("snakeoil.pem"),
        &target_path,
        &newer_root_repo,
        2,
    )
    .await
    .expect("sign repository for the new-root pass");
    let metadata_dir = newer_root_repo.join("metadata");
    next_root
        .write(&metadata_dir, original_root.signed.consistent_snapshot)
        .await
        .expect("write valid next-root metadata");

    let first = load_from_directories(
        &root_bytes,
        &metadata_dir,
        &newer_root_repo.join("targets"),
        Some(&datastore),
    )
    .await
    .expect("load valid next-root chain");
    assert_eq!(u64::from(first.root().signed.version), 2);

    write_signed_repository(
        &root_path,
        &fixtures.join("snakeoil.pem"),
        &target_path,
        &stale_root_repo,
        3,
    )
    .await
    .expect("sign newer timestamp metadata still valid under the old root");
    let second = load_from_directories(
        &root_bytes,
        &stale_root_repo.join("metadata"),
        &stale_root_repo.join("targets"),
        Some(&datastore),
    )
    .await
    .expect("demonstrate bootstrap replay with monotonic timestamp metadata");
    assert_eq!(u64::from(second.root().signed.version), 1);
    assert_eq!(u64::from(second.timestamp().signed.version), 3);
}
