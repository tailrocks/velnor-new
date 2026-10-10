use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::time::{Instant, sleep};

use crate::error::HostError;

use super::super::path::MAX_HELPER_BYTES;
use super::{image, validate_open_file, validate_path_file};

const MAINTENANCE_LOCK: &str = ".maintenance.lock";
const SNAPSHOT_SUFFIX: &str = ".helper";
const LOCK_SUFFIX: &str = ".lock";
const PARTIAL_PREFIX: &str = ".partial-";
const MAX_STORE_ENTRIES: usize = 32;
const MAX_STORE_BYTES: u64 = 8 * MAX_HELPER_BYTES;
const MAINTENANCE_WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct Inventory {
    pub(super) snapshots: BTreeMap<String, SnapshotPair>,
    partials: Vec<PathBuf>,
    entries: usize,
    bytes: u64,
}

#[derive(Default)]
pub(super) struct SnapshotPair {
    pub(super) image: Option<PathBuf>,
    lock: Option<PathBuf>,
}

pub(super) async fn acquire_maintenance_lock(
    directory: &Path,
    owner: u32,
) -> Result<File, HostError> {
    let path = directory.join(MAINTENANCE_LOCK);
    let file = open_control_file(&path, owner, true)?;
    let deadline = Instant::now() + MAINTENANCE_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                sleep(Duration::from_millis(10)).await;
            }
            Err(_) => return Err(HostError::Lock),
        }
    }
}

pub(super) fn collect_inventory(directory: &Path, owner: u32) -> Result<Inventory, HostError> {
    let mut inventory = Inventory::default();
    for item in fs::read_dir(directory).map_err(|_| HostError::Path)? {
        let item = item.map_err(|_| HostError::Path)?;
        let path = item.path();
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| HostError::Path)?;
        if name == MAINTENANCE_LOCK {
            validate_path_file(&path, owner, 0o600, 0)?;
            continue;
        }
        inventory.entries = inventory.entries.checked_add(1).ok_or(HostError::Frame)?;
        if inventory.entries > MAX_STORE_ENTRIES {
            return Err(HostError::Frame);
        }
        record_entry(&mut inventory, &name, path, owner)?;
    }
    for (digest, pair) in &inventory.snapshots {
        if pair.image.is_some() && pair.lock.is_none() {
            return Err(HostError::Path);
        }
        valid_digest(digest)?;
    }
    if inventory.bytes > MAX_STORE_BYTES {
        return Err(HostError::Frame);
    }
    Ok(inventory)
}

fn record_entry(
    inventory: &mut Inventory,
    name: &str,
    path: PathBuf,
    owner: u32,
) -> Result<(), HostError> {
    if let Some(nonce) = name.strip_prefix(PARTIAL_PREFIX) {
        if uuid::Uuid::parse_str(nonce).map_or(true, |value| value.to_string() != nonce) {
            return Err(HostError::Path);
        }
        validate_partial_path(&path, owner)?;
        add_size(inventory, &path)?;
        inventory.partials.push(path);
        return Ok(());
    }
    if let Some(digest) = name.strip_suffix(SNAPSHOT_SUFFIX) {
        valid_digest(digest)?;
        let metadata = validate_path_file(&path, owner, 0o500, MAX_HELPER_BYTES)?;
        inventory.bytes = inventory
            .bytes
            .checked_add(metadata.len())
            .ok_or(HostError::Frame)?;
        inventory
            .snapshots
            .entry(digest.to_owned())
            .or_default()
            .image = Some(path);
        return Ok(());
    }
    if let Some(digest) = name.strip_suffix(LOCK_SUFFIX) {
        valid_digest(digest)?;
        validate_path_file(&path, owner, 0o600, 0)?;
        inventory
            .snapshots
            .entry(digest.to_owned())
            .or_default()
            .lock = Some(path);
        return Ok(());
    }
    Err(HostError::Path)
}

fn add_size(inventory: &mut Inventory, path: &Path) -> Result<(), HostError> {
    let size = fs::symlink_metadata(path)
        .map_err(|_| HostError::Path)?
        .len();
    inventory.bytes = inventory.bytes.checked_add(size).ok_or(HostError::Frame)?;
    Ok(())
}

pub(super) fn validate_inventory(inventory: &Inventory, owner: u32) -> Result<(), HostError> {
    for (digest, pair) in &inventory.snapshots {
        if let Some(path) = pair.image.as_ref() {
            let mut image = image::open_image(path, owner)?;
            if image::hex_digest(&image::hash_file(&mut image)?) != *digest {
                return Err(HostError::Identity);
            }
        }
    }
    Ok(())
}

pub(super) fn prune_stale(
    inventory: &Inventory,
    expected_sha256: &[u8; 32],
    owner: u32,
) -> Result<(), HostError> {
    let current = image::digest_name(expected_sha256);
    for partial in &inventory.partials {
        remove_partial(partial, owner)?;
    }
    for (digest, pair) in &inventory.snapshots {
        if digest == &current && pair.image.is_some() {
            continue;
        }
        let lock_path = pair.lock.as_ref().ok_or(HostError::Path)?;
        let lock = open_control_file(lock_path, owner, false)?;
        match lock.try_lock() {
            Ok(()) => remove_snapshot_pair(pair, &lock, owner, digest)?,
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(_) => return Err(HostError::Lock),
        }
    }
    Ok(())
}

fn remove_snapshot_pair(
    pair: &SnapshotPair,
    lock: &File,
    owner: u32,
    digest: &str,
) -> Result<(), HostError> {
    let lock_path = pair.lock.as_ref().ok_or(HostError::Path)?;
    validate_open_file(lock_path, lock, owner, 0o600, 0)?;
    if let Some(image_path) = pair.image.as_ref() {
        let mut image = image::open_image(image_path, owner)?;
        if image::hex_digest(&image::hash_file(&mut image)?) != digest {
            return Err(HostError::Identity);
        }
        validate_path_file(image_path, owner, 0o500, MAX_HELPER_BYTES)?;
        fs::remove_file(image_path).map_err(|_| HostError::Path)?;
    }
    fs::remove_file(lock_path).map_err(|_| HostError::Path)
}

fn remove_partial(path: &Path, owner: u32) -> Result<(), HostError> {
    validate_partial_path(path, owner)?;
    fs::remove_file(path).map_err(|_| HostError::Path)
}

fn validate_partial_path(path: &Path, owner: u32) -> Result<(), HostError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    let mode = metadata.mode() & 0o7777;
    if mode != 0o500 && mode != 0o600 {
        return Err(HostError::Path);
    }
    validate_path_file(path, owner, mode, MAX_HELPER_BYTES)?;
    Ok(())
}

pub(super) fn ensure_room(inventory: &Inventory, size: u64) -> Result<(), HostError> {
    if inventory
        .entries
        .checked_add(2)
        .is_none_or(|count| count > MAX_STORE_ENTRIES)
        || inventory
            .bytes
            .checked_add(size)
            .is_none_or(|total| total > MAX_STORE_BYTES)
    {
        return Err(HostError::Frame);
    }
    Ok(())
}

pub(super) fn create_snapshot_lock(path: &Path, owner: u32) -> Result<File, HostError> {
    create_control_file(path, owner)
}

pub(super) fn open_snapshot_shared_lock(path: &Path, owner: u32) -> Result<File, HostError> {
    let file = open_control_file(path, owner, false)?;
    file.try_lock_shared().map_err(|_| HostError::Lock)?;
    Ok(file)
}

fn create_control_file(path: &Path, owner: u32) -> Result<File, HostError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(path).map_err(|_| HostError::Path)?;
    validate_open_file(path, &file, owner, 0o600, 0)?;
    Ok(file)
}

fn open_control_file(path: &Path, owner: u32, create: bool) -> Result<File, HostError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    if create {
        options.create(true).mode(0o600);
    }
    let file = options.open(path).map_err(|_| HostError::Path)?;
    validate_open_file(path, &file, owner, 0o600, 0)?;
    Ok(file)
}

fn valid_digest(digest: &str) -> Result<(), HostError> {
    if digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(HostError::Path)
    }
}
