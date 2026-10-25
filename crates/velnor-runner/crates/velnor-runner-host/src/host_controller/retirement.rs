//! Recover drained connection retirement after a controller restart.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use crate::config::{ConnectionId, HostConfig, RepositoryConnection};
use crate::controller_ipc::ControlState;
use crate::daemon_lock::DaemonLock;
use crate::error::HostError;
use crate::journal::Journal;

const MAX_BINDING_BYTES: u64 = 1_048_576;
const MAX_DRAIN_PROOF_BYTES: u64 = MAX_BINDING_BYTES + 33;
const MAX_SIDECARS: usize = 4096;

/// Find exact, drained bindings removed from the current configuration.
///
/// The CLI writes these sidecars before removing a binding from `host.toml`.
/// A caller must still recheck both the current config and proof under
/// `admission.lock` before retiring the journal scope.
pub(super) fn removed_bindings(
    state: &Path,
    config: &HostConfig,
) -> Result<Vec<RepositoryConnection>, HostError> {
    let directory = state.join("connection-drain");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Ok(_) | Err(_) => return Err(HostError::Path),
    };
    if !trusted_private_directory(&metadata) {
        return Err(HostError::Path);
    }

    let entries = fs::read_dir(&directory).map_err(|_| HostError::Path)?;
    let mut bindings = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_SIDECARS {
            return Err(HostError::Path);
        }
        let entry = entry.map_err(|_| HostError::Path)?;
        let file_name = entry.file_name();
        let Some(id_text) = file_name
            .to_str()
            .and_then(|name| name.strip_suffix("~binding"))
        else {
            continue;
        };
        let Ok(id) = ConnectionId::parse(id_text) else {
            continue;
        };
        if config.connection(&id).is_some() {
            continue;
        }
        let snapshot_path = entry.path();
        let Some(snapshot) = read_regular_file(&snapshot_path, MAX_BINDING_BYTES)? else {
            continue;
        };
        let Some(binding) = parse_binding(&snapshot) else {
            continue;
        };
        if binding.connection_id != id {
            continue;
        }
        let marker_path = directory.join(id.as_str());
        let Some(token) = read_token(&marker_path)? else {
            continue;
        };
        let proof_path = directory.join(format!("{}~drained", id.as_str()));
        let Some(proof) = read_regular_file(&proof_path, MAX_DRAIN_PROOF_BYTES)? else {
            continue;
        };
        if !proof_matches(&proof, &token, &snapshot) {
            continue;
        }
        if !bindings.iter().any(|existing| existing == &binding) {
            bindings.push(binding);
        }
    }
    Ok(bindings)
}

/// Verify the current marker and its exact binding proof again before a mutation.
pub(super) fn proof_is_current(
    state: &Path,
    binding: &RepositoryConnection,
) -> Result<bool, HostError> {
    let directory = state.join("connection-drain");
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Ok(_) | Err(_) => return Err(HostError::Path),
    };
    if !trusted_private_directory(&metadata) {
        return Err(HostError::Path);
    }
    let id = binding.connection_id.as_str();
    let marker_path = directory.join(id);
    let Some(token) = read_token(&marker_path)? else {
        return Ok(false);
    };
    let snapshot_path = directory.join(format!("{id}~binding"));
    let Some(snapshot) = read_regular_file(&snapshot_path, MAX_BINDING_BYTES)? else {
        return Ok(false);
    };
    if snapshot
        != binding
            .canonical_identity_bytes()
            .map_err(|_| HostError::Config)?
    {
        return Ok(false);
    }
    let proof_path = directory.join(format!("{id}~drained"));
    let Some(proof) = read_regular_file(&proof_path, MAX_DRAIN_PROOF_BYTES)? else {
        return Ok(false);
    };
    Ok(proof_matches(&proof, &token, &snapshot))
}

/// Retire only after config absence and the controller's published view agree under the
/// same lifecycle lock used by CLI config changes.
pub(super) async fn retire_if_removed(
    state: &Path,
    journal: &Journal,
    control_state: &ControlState,
    binding: &RepositoryConnection,
) -> Result<bool, HostError> {
    let _admission = DaemonLock::try_acquire(&state.join("admission.lock"))?;
    if control_state.contains_connection(binding)? {
        return Ok(false);
    }
    let Some(config) = super::config_watch::read_host_config(&state.join("host.toml"))? else {
        return Ok(false);
    };
    if config.connection(&binding.connection_id).is_some() {
        return Ok(false);
    }
    if has_binding_snapshot(state, binding)? && !proof_is_current(state, binding)? {
        return Ok(false);
    }
    journal.retire_connection_if_drained(binding).await
}

fn parse_binding(bytes: &[u8]) -> Option<RepositoryConnection> {
    let binding: RepositoryConnection = serde_json::from_slice(bytes).ok()?;
    (binding.canonical_identity_bytes().ok()?.as_slice() == bytes).then_some(binding)
}

fn proof_matches(proof: &[u8], token: &str, snapshot: &[u8]) -> bool {
    proof.len() == token.len() + 1 + snapshot.len()
        && proof.get(..32) == Some(token.as_bytes())
        && proof.get(32) == Some(&b'\n')
        && proof.get(33..) == Some(snapshot)
}

fn read_token(path: &Path) -> Result<Option<String>, HostError> {
    let Some(bytes) = read_regular_file(path, 32)? else {
        return Ok(None);
    };
    if bytes.len() != 32 || !bytes.iter().all(u8::is_ascii_hexdigit) {
        return Ok(None);
    }
    if bytes.iter().any(u8::is_ascii_uppercase) {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| HostError::Path)
}

fn read_regular_file(path: &Path, max_bytes: u64) -> Result<Option<Vec<u8>>, HostError> {
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(_) | Err(_) => return Err(HostError::Path),
    };
    if before.len() > max_bytes || !trusted_private_file(&before) {
        return Err(HostError::Path);
    }
    let mut file = open_nofollow(path)?;
    let opened = file.metadata().map_err(|_| HostError::Path)?;
    if !same_file(&before, &opened)
        || !opened.is_file()
        || opened.len() > max_bytes
        || !trusted_private_file(&opened)
    {
        return Err(HostError::Path);
    }
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(0)).map_err(|_| HostError::Path)?;
    (&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| HostError::Path)?;
    let after = file_metadata(path, &file, &opened)?;
    if !same_file(&opened, &after)
        || u64::try_from(bytes.len()).map_err(|_| HostError::Path)? != after.len()
        || after.len() > max_bytes
        || !trusted_private_file(&after)
    {
        return Err(HostError::Path);
    }
    Ok(Some(bytes))
}

fn open_nofollow(path: &Path) -> Result<File, HostError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    options.open(path).map_err(|_| HostError::Path)
}

fn file_metadata(
    path: &Path,
    file: &File,
    opened: &fs::Metadata,
) -> Result<fs::Metadata, HostError> {
    let descriptor = file.metadata().map_err(|_| HostError::Path)?;
    let path = fs::symlink_metadata(path).map_err(|_| HostError::Path)?;
    if !same_file(opened, &path) {
        return Err(HostError::Path);
    }
    Ok(descriptor)
}

fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn trusted_private_directory(metadata: &fs::Metadata) -> bool {
    metadata.uid() == rustix::process::geteuid().as_raw() && metadata.mode().trailing_zeros() >= 6
}

fn trusted_private_file(metadata: &fs::Metadata) -> bool {
    metadata.uid() == rustix::process::geteuid().as_raw() && metadata.mode().trailing_zeros() >= 6
}

/// Whether a CLI binding snapshot exists and therefore requires its matching drain proof.
pub(super) fn has_binding_snapshot(
    state: &Path,
    binding: &RepositoryConnection,
) -> Result<bool, HostError> {
    let path = state
        .join("connection-drain")
        .join(format!("{}~binding", binding.connection_id.as_str()));
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Ok(_) | Err(_) => Err(HostError::Path),
    }
}
