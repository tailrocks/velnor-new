//! Durable identity and exclusive lifetime for one journal's resource probe.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use crate::error::HostError;
use crate::journal::Journal;

const LOCK_SUFFIX: &str = ".velnor-guest-probe.lock";
const PRIVATE_MODE: u32 = 0o600;

/// Lease held until the guest sampler has cleaned up and its thread has exited.
#[must_use = "retain the guest probe lease through sampler cleanup"]
pub(crate) struct GuestProbeLease {
    token: String,
    _lock: File,
}

impl fmt::Debug for GuestProbeLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GuestProbeLease")
            .field("token", &"[redacted]")
            .field("lock", &"[held]")
            .finish()
    }
}

impl GuestProbeLease {
    /// The opaque journal-scoped owner token used in probe labels.
    #[must_use]
    pub(crate) fn token(&self) -> &str {
        &self.token
    }
}

impl Journal {
    /// Claim exclusive sampler ownership and load the durable journal token.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Lock`] for contention or an unsafe lock file,
    /// [`HostError::Path`] for a redirected database path, or
    /// [`HostError::Journal`] for malformed or unreadable durable state.
    pub(crate) async fn claim_guest_probe_owner(&self) -> Result<GuestProbeLease, HostError> {
        let lock = acquire_lock(self.file.path())?;
        let token = self.guest_probe_owner_token().await?;
        if !valid_owner_token(&token) {
            return Err(HostError::Journal);
        }
        Ok(GuestProbeLease { token, _lock: lock })
    }

    /// Read or atomically initialize this journal's opaque probe owner token.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for malformed state or failed SQLite or
    /// random-number operations.
    pub(crate) async fn guest_probe_owner_token(&self) -> Result<String, HostError> {
        let connection = self.connection().await?;
        connection
            .execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let result = get_or_create_token(&connection).await;
        match result {
            Ok(token) => match connection.execute("COMMIT", ()).await {
                Ok(_) => Ok(token),
                Err(_) => {
                    rollback(&connection).await?;
                    Err(HostError::Journal)
                }
            },
            Err(error) => {
                rollback(&connection).await?;
                Err(error)
            }
        }
    }
}

async fn get_or_create_token(connection: &turso::Connection) -> Result<String, HostError> {
    if let Some(token) = read_owner_token(connection).await? {
        return Ok(token);
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| HostError::Journal)?;
    let token = lower_hex(&random);
    let inserted = connection
        .execute(
            "INSERT INTO guest_probe_owner (id, owner_token) VALUES (1, ?1)",
            [token.clone()],
        )
        .await
        .map_err(|_| HostError::Journal)?;
    if inserted != 1 || read_owner_token(connection).await?.as_deref() != Some(&token) {
        return Err(HostError::Journal);
    }
    Ok(token)
}

async fn read_owner_token(connection: &turso::Connection) -> Result<Option<String>, HostError> {
    let mut rows = connection
        .query("SELECT id, owner_token FROM guest_probe_owner", ())
        .await
        .map_err(|_| HostError::Journal)?;
    let Some(row) = rows.next().await.map_err(|_| HostError::Journal)? else {
        return Ok(None);
    };
    let id: i64 = row.get(0).map_err(|_| HostError::Journal)?;
    let token: String = row.get(1).map_err(|_| HostError::Journal)?;
    if id != 1
        || !valid_owner_token(&token)
        || rows.next().await.map_err(|_| HostError::Journal)?.is_some()
    {
        return Err(HostError::Journal);
    }
    Ok(Some(token))
}

async fn rollback(connection: &turso::Connection) -> Result<(), HostError> {
    connection
        .execute("ROLLBACK", ())
        .await
        .map(|_| ())
        .map_err(|_| HostError::Journal)
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        token.push(char::from(HEX[usize::from(*byte >> 4)]));
        token.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    token
}

fn acquire_lock(database: &Path) -> Result<File, HostError> {
    let canonical = fs::canonicalize(database).map_err(|_| HostError::Path)?;
    if canonical != database || !canonical.is_absolute() {
        return Err(HostError::Path);
    }
    let database_metadata = fs::symlink_metadata(&canonical).map_err(|_| HostError::Path)?;
    if database_metadata.file_type().is_symlink() || !database_metadata.is_file() {
        return Err(HostError::Path);
    }
    let parent = canonical.parent().ok_or(HostError::Path)?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|_| HostError::Path)?;
    if !parent_metadata.is_dir() || parent_metadata.mode() & 0o022 != 0 {
        return Err(HostError::Lock);
    }
    let path = lock_path(&canonical)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(PRIVATE_MODE)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| HostError::Lock)?;
    let metadata = lock.metadata().map_err(|_| HostError::Lock)?;
    if !metadata.is_file()
        || metadata.mode() & 0o777 != PRIVATE_MODE
        || metadata.nlink() != 1
        || metadata.uid() != database_metadata.uid()
    {
        return Err(HostError::Lock);
    }
    lock.try_lock().map_err(|_| HostError::Lock)?;
    Ok(lock)
}

fn lock_path(database: &Path) -> Result<PathBuf, HostError> {
    let parent = database.parent().ok_or(HostError::Path)?;
    let name = database.file_name().ok_or(HostError::Path)?;
    let mut lock_name = name.to_os_string();
    lock_name.push(LOCK_SUFFIX);
    Ok(parent.join(lock_name))
}

fn valid_owner_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "guest_probe_owner_tests.rs"]
mod tests;
