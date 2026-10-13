//! Bounded redaction and durable storage for one runner's `_diag` archive.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Component, Path};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tar::{Archive, Builder, EntryType, Header};
use zeroize::Zeroizing;

use crate::HostError;

use super::{PostActionDisposition, WorkerGenerationIdentity};
use filesystem::{
    atomic_write_at, digest_hex, effective_uid, open_private_directory_at, read_private_file_at,
    require_absent_at, sync_directory, validate_private_directory, verify_digest,
};

mod filesystem;
mod state_directory;

pub use state_directory::{
    ProtectedStateDirectory, ProtectedStateDirectoryIdentity, validate_protected_state_directory,
};

const MAX_DIAGNOSTIC_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_DIAGNOSTIC_LOG_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIAGNOSTIC_FILES: usize = 256;
const MAX_RECEIPT_BYTES: usize = 16 * 1024;

/// Durable host path and digest for redacted runner diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsReceipt {
    relative_path: String,
    sha256: String,
    bytes: u64,
    redacted: bool,
    retained: bool,
    source_absent: bool,
}

impl DiagnosticsReceipt {
    /// Relative path beneath the configured host-controlled diagnostic root.
    #[must_use]
    pub fn relative_path(&self) -> &str {
        &self.relative_path
    }

    /// SHA-256 of the retained archive, or empty bytes for a never-started worker.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Number of redacted archive bytes retained.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// True only after the redaction pass completed successfully.
    #[must_use]
    pub const fn redacted(&self) -> bool {
        self.redacted
    }

    /// True only after data and metadata were synced to the host filesystem.
    #[must_use]
    pub const fn retained(&self) -> bool {
        self.retained
    }

    /// True when the generation never produced a `_diag` source directory.
    #[must_use]
    pub const fn source_absent(&self) -> bool {
        self.source_absent
    }
}

/// Restricted directory used only for retained runner diagnostics.
#[derive(Debug, Clone)]
pub struct DiagnosticsStore {
    root_directory: Arc<File>,
    owner: u32,
}

mod store;

impl DiagnosticsStore {
    /// Open an existing absolute mode-0700 directory owned by the service UID.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Path`] when the directory is absent, unsafe, or cannot be inspected.
    pub fn new(root: &Path) -> Result<Self, HostError> {
        let owner = effective_uid();
        let directory = filesystem::open_trusted_directory(root, owner)?;
        validate_private_directory(&directory, owner)?;
        Ok(Self {
            root_directory: Arc::new(directory),
            owner,
        })
    }

    /// Recover a previously written receipt only when identity and digest match.
    pub(super) fn load(
        &self,
        identity: &WorkerGenerationIdentity,
        post_actions: &PostActionDisposition,
    ) -> Result<Option<DiagnosticsReceipt>, HostError> {
        let directory_name = launch_directory_name(identity.launch_id());
        let Some(directory) =
            open_private_directory_at(&self.root_directory, &directory_name, self.owner, false)?
        else {
            return Ok(None);
        };
        let Some(bytes) =
            read_private_file_at(&directory, "receipt.json", self.owner, MAX_RECEIPT_BYTES)?
        else {
            return Ok(None);
        };
        let stored: StoredReceipt = serde_json::from_slice(&bytes).map_err(|_| HostError::Path)?;
        if stored.launch_id != identity.launch_id()
            || stored.worker_volume != identity.worker_volume()
            || stored.runner_container_id != identity.runner_container_id()
            || stored.dind_container_id != identity.dind_container_id()
            || stored.post_actions != *post_actions
            || !stored.receipt.redacted
            || !stored.receipt.retained
            || stored.receipt.source_absent != matches!(post_actions, PostActionDisposition::NotRun)
        {
            return Err(HostError::Identity);
        }
        if stored.receipt.source_absent {
            if !stored.receipt.relative_path.is_empty()
                || stored.receipt.sha256 != digest_hex(&[])
                || stored.receipt.bytes != 0
            {
                return Err(HostError::Identity);
            }
            require_absent_at(&directory, "runner-diagnostics.tar")?;
            return Ok(Some(stored.receipt));
        }
        if stored.receipt.relative_path
            != format!("launch-{}/runner-diagnostics.tar", identity.launch_id())
        {
            return Err(HostError::Identity);
        }
        let archive = read_private_file_at(
            &directory,
            "runner-diagnostics.tar",
            self.owner,
            MAX_DIAGNOSTIC_ARCHIVE_BYTES,
        )?
        .ok_or(HostError::Path)?;
        verify_digest(&archive, &stored.receipt)?;
        Ok(Some(stored.receipt))
    }

    /// Redact, persist, and checksum the exact runner diagnostics before cleanup.
    pub(super) fn retain(
        &self,
        identity: &WorkerGenerationIdentity,
        post_actions: &PostActionDisposition,
        raw_archive: Option<&[u8]>,
    ) -> Result<DiagnosticsReceipt, HostError> {
        let source_absent = raw_archive.is_none();
        if source_absent && !matches!(post_actions, PostActionDisposition::NotRun) {
            return Err(HostError::Docker);
        }
        if !source_absent && matches!(post_actions, PostActionDisposition::NotRun) {
            return Err(HostError::Identity);
        }
        if raw_archive.is_some_and(|bytes| bytes.len() > MAX_DIAGNOSTIC_ARCHIVE_BYTES) {
            return Err(HostError::Frame);
        }
        let directory_name = launch_directory_name(identity.launch_id());
        let directory =
            open_private_directory_at(&self.root_directory, &directory_name, self.owner, true)?
                .ok_or(HostError::Path)?;
        let receipt = if source_absent {
            require_absent_at(&directory, "runner-diagnostics.tar")?;
            DiagnosticsReceipt {
                relative_path: String::new(),
                sha256: digest_hex(&[]),
                bytes: 0,
                redacted: true,
                retained: true,
                source_absent: true,
            }
        } else {
            let sanitized = sanitize_archive(raw_archive.ok_or(HostError::Docker)?)?;
            let digest = digest_hex(&sanitized);
            atomic_write_at(&directory, "runner-diagnostics.tar", self.owner, &sanitized)?;
            DiagnosticsReceipt {
                relative_path: format!("launch-{}/runner-diagnostics.tar", identity.launch_id()),
                sha256: digest,
                bytes: u64::try_from(sanitized.len()).map_err(|_| HostError::Frame)?,
                redacted: true,
                retained: true,
                source_absent: false,
            }
        };
        let stored = StoredReceipt {
            launch_id: identity.launch_id(),
            worker_volume: identity.worker_volume().to_owned(),
            runner_container_id: identity.runner_container_id().to_owned(),
            dind_container_id: identity.dind_container_id().to_owned(),
            post_actions: post_actions.clone(),
            receipt: receipt.clone(),
        };
        let manifest = serde_json::to_vec(&stored).map_err(|_| HostError::Path)?;
        atomic_write_at(&directory, "receipt.json", self.owner, &manifest)?;
        sync_directory(&directory)?;
        Ok(receipt)
    }
}

fn launch_directory_name(launch_id: i64) -> String {
    format!("launch-{launch_id}")
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredReceipt {
    launch_id: i64,
    worker_volume: String,
    runner_container_id: String,
    dind_container_id: String,
    post_actions: PostActionDisposition,
    receipt: DiagnosticsReceipt,
}

fn sanitize_archive(raw_archive: &[u8]) -> Result<Vec<u8>, HostError> {
    let mut output = Vec::new();
    let mut builder = Builder::new(&mut output);
    let mut files = 0_usize;
    let mut source_bytes = 0_usize;
    let mut archive = Archive::new(Cursor::new(raw_archive));
    for entry in archive.entries().map_err(|_| HostError::Docker)? {
        let entry = entry.map_err(|_| HostError::Docker)?;
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            continue;
        }
        if kind != EntryType::Regular {
            return Err(HostError::Docker);
        }
        let path = entry.path().map_err(|_| HostError::Docker)?;
        if !safe_archive_path(&path) {
            return Err(HostError::Docker);
        }
        if path.extension() != Some(OsStr::new("log")) {
            continue;
        }
        files = files.saturating_add(1);
        if files > MAX_DIAGNOSTIC_FILES {
            return Err(HostError::Frame);
        }
        let declared = usize::try_from(entry.size()).map_err(|_| HostError::Frame)?;
        if declared > MAX_DIAGNOSTIC_LOG_BYTES {
            return Err(HostError::Frame);
        }
        source_bytes = source_bytes.saturating_add(declared);
        if source_bytes > MAX_DIAGNOSTIC_ARCHIVE_BYTES {
            return Err(HostError::Frame);
        }
        let mut raw = Zeroizing::new(Vec::with_capacity(declared));
        entry
            .take(MAX_DIAGNOSTIC_LOG_BYTES as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|_| HostError::Docker)?;
        if raw.len() != declared {
            return Err(HostError::Docker);
        }
        let redacted = redact_log(&raw)?;
        append_log(&mut builder, files, &redacted)?;
    }
    if files == 0 {
        return Err(HostError::Docker);
    }
    builder.finish().map_err(|_| HostError::Docker)?;
    drop(builder);
    if output.len() > MAX_DIAGNOSTIC_ARCHIVE_BYTES {
        return Err(HostError::Frame);
    }
    Ok(output)
}

fn safe_archive_path(path: &Path) -> bool {
    let mut names = 0_usize;
    for component in path.components() {
        match component {
            Component::Normal(_) => names += 1,
            Component::CurDir => {}
            _ => return false,
        }
    }
    names > 0 && names <= 8
}

fn append_log(
    builder: &mut Builder<&mut Vec<u8>>,
    index: usize,
    contents: &[u8],
) -> Result<(), HostError> {
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_size(u64::try_from(contents.len()).map_err(|_| HostError::Frame)?);
    header.set_mode(0o600);
    header.set_mtime(0);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();
    builder
        .append_data(&mut header, format!("runner-{index:03}.log"), contents)
        .map_err(|_| HostError::Docker)
}

fn redact_log(bytes: &[u8]) -> Result<Vec<u8>, HostError> {
    let text = std::str::from_utf8(bytes).map_err(|_| HostError::Docker)?;
    let mut output = Vec::with_capacity(bytes.len());
    for line in text.split_inclusive('\n') {
        let lower = line.to_ascii_lowercase();
        if [
            "authorization",
            "access_token",
            "refresh_token",
            "runnercredential",
            "credential",
            "password",
            "secret",
            "jitconfig",
            "bearer ",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
        {
            output.extend_from_slice(b"[REDACTED sensitive diagnostic line]\n");
            continue;
        }
        redact_long_tokens(line.as_bytes(), &mut output);
        if output.len() > MAX_DIAGNOSTIC_LOG_BYTES {
            return Err(HostError::Frame);
        }
    }
    Ok(output)
}

fn redact_long_tokens(input: &[u8], output: &mut Vec<u8>) {
    let mut index = 0;
    while index < input.len() {
        if is_token_byte(input[index]) {
            let start = index;
            while index < input.len() && is_token_byte(input[index]) {
                index += 1;
            }
            if index - start >= 32 {
                output.extend_from_slice(b"[REDACTED token]");
            } else {
                output.extend_from_slice(&input[start..index]);
            }
        } else {
            output.push(input[index]);
            index += 1;
        }
    }
}

const fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'/' | b'=')
}

#[cfg(test)]
mod tests;
