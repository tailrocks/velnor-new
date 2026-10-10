use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tempfile::TempDir;

use super::*;

pub(super) fn root_fixture() -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tough/simple-rsa/root.json"),
    )
    .expect("read signed fixture root")
}

pub(super) fn seal_stage(generation: &StagedGeneration, bootstrap: &[u8]) {
    let identity = RootIdentity::from_bytes(bootstrap).expect("parse fixture root");
    let root_path = generation.path.join(TUF_DIR).join("root.json");
    if !root_path.exists() {
        write_file(&root_path, bootstrap, 0o600).expect("write staged root");
    }
    let state = RootHighWater {
        schema: 1,
        bootstrap_sha256: sha256_hex(bootstrap),
        bootstrap_version: identity.version,
        bootstrap_signed_sha256: identity.signed_sha256.clone(),
        root_version: identity.version,
        root_signed_sha256: identity.signed_sha256,
        tuf_files: hash_tuf_files(&generation.path.join(TUF_DIR)).expect("hash staged TUF files"),
    };
    write_file(
        &generation.path.join(STATE_FILE),
        &serde_json::to_vec(&state).expect("serialize root state"),
        0o600,
    )
    .expect("write staged high-water state");
    let manifest = GenerationManifest {
        schema: 1,
        generation: generation.id.clone(),
        files: hash_tree_files(&generation.path).expect("hash generation files"),
    };
    write_file(
        &generation.path.join(MANIFEST_FILE),
        &serde_json::to_vec(&manifest).expect("serialize generation manifest"),
        0o600,
    )
    .expect("write staged manifest");
    sync_tree(&generation.path).expect("sync staged generation");
}

pub(super) fn generation_path(root: &Path, id: &str) -> PathBuf {
    root.join(GENERATIONS_DIR).join(id)
}

#[tokio::test]
async fn active_pointer_is_atomic_across_unactivated_generation() {
    let temporary = TempDir::new().expect("create isolated cache root");
    let cache_root = fs::canonicalize(temporary.path())
        .expect("canonicalize temporary root")
        .join("cache");
    let cache = TufCache::new(&cache_root);
    cache.prepare_root().expect("prepare private cache root");
    let bootstrap = root_fixture();

    let first = cache.create_staging(None).expect("create first generation");
    seal_stage(&first, &bootstrap);
    cache.publish(&first).expect("publish initial generation");
    let first_id = first.id.clone();
    assert_eq!(
        cache
            .read_active()
            .expect("read first generation")
            .expect("active generation is present")
            .id,
        first_id
    );

    let active = cache
        .read_active()
        .expect("read active generation")
        .expect("active generation is present");
    let second = cache
        .create_staging(Some(&active))
        .expect("create second generation from active TUF metadata");
    seal_stage(&second, &bootstrap);
    let second_id = second.id.clone();
    cache
        .place_generation(&second)
        .expect("durably place complete inactive generation");

    assert!(generation_path(&cache.root, &second_id).is_dir());
    assert_eq!(
        cache
            .read_active()
            .expect("old generation remains active")
            .expect("active generation is present")
            .id,
        first_id
    );
    assert_eq!(
        fs::read_to_string(cache.root.join(CURRENT_FILE))
            .expect("read CURRENT")
            .trim(),
        first_id
    );

    cache
        .activate_generation(&second_id)
        .expect("atomically swap CURRENT");
    assert_eq!(
        cache
            .read_active()
            .expect("read activated generation")
            .expect("active generation is present")
            .id,
        second_id
    );
}

#[tokio::test]
async fn cache_lock_serializes_independent_open_handles() {
    let temporary = TempDir::new().expect("create isolated cache root");
    let cache_root = fs::canonicalize(temporary.path())
        .expect("canonicalize temporary root")
        .join("cache");
    let cache = TufCache::new(&cache_root);
    cache.prepare_root().expect("prepare private cache root");
    let first = CacheLock::acquire(&cache.root, Duration::from_secs(1))
        .await
        .expect("acquire first cache lock");
    let root = cache.root.clone();
    let waiter =
        tokio::spawn(async move { CacheLock::acquire(&root, Duration::from_secs(2)).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !waiter.is_finished(),
        "second handle must wait for the lock"
    );
    drop(first);
    let second = tokio::time::timeout(Duration::from_secs(1), waiter)
        .await
        .expect("second handle acquires after release")
        .expect("lock task joins")
        .expect("second lock succeeds");
    drop(second);
}

#[test]
fn existing_permissive_cache_directory_is_rejected_without_chmod_repair() {
    let temporary = TempDir::new().expect("create isolated parent");
    let root = fs::canonicalize(temporary.path())
        .expect("canonicalize temporary parent")
        .join("cache");
    fs::create_dir(&root).expect("create permissive cache dir");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755))
        .expect("set deliberately permissive mode");
    let cache = TufCache::new(&root);
    assert!(cache.prepare_root().is_err());
    assert_eq!(
        fs::symlink_metadata(&root)
            .expect("inspect cache directory")
            .permissions()
            .mode()
            & 0o777,
        0o755,
        "unsafe pre-existing state must not be silently repaired"
    );
}

#[test]
fn cache_directory_owner_must_match_the_effective_service_uid() {
    let temporary = TempDir::new().expect("create isolated cache root");
    let path = fs::canonicalize(temporary.path()).expect("canonicalize private directory");
    let current_uid = effective_uid();
    assert!(validate_private_directory_path_for_uid(&path, current_uid).is_ok());
    assert!(validate_private_directory_path_for_uid(&path, current_uid.wrapping_add(1)).is_err());
}

#[cfg(unix)]
#[test]
fn cache_path_rejects_a_symlinked_parent_component() {
    use std::os::unix::fs::symlink;

    let temporary = TempDir::new().expect("create isolated cache root");
    let real_parent = fs::canonicalize(temporary.path()).expect("canonicalize private parent");
    let alias = real_parent.join("alias");
    symlink(&real_parent, &alias).expect("create symlinked parent");
    let cache = TufCache::new(alias.join("cache"));
    assert!(cache.prepare_root().is_err());
    assert!(!alias.join("cache").exists());
}
