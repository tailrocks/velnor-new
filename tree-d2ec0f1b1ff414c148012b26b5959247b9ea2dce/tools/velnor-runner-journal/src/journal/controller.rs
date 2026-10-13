//! Read-only inspection and durable controller admission state.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;

use super::Journal;

impl Journal {
    /// Open an existing journal without running schema bootstrap or permitting writes.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the path is missing, not a regular file,
    /// a symlink, or cannot be opened read-only.
    pub async fn open_readonly(path: &Path) -> Result<Self, HostError> {
        existing_regular_file(path)?;
        let journal = Self {
            path: path.to_path_buf(),
            read_only: true,
            protected_path: None,
        };
        let _connection = journal.connection().await?;
        Ok(journal)
    }

    /// Open an existing journal for state changes without creating or migrating it.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the path is missing, not a regular file,
    /// a symlink, or cannot be opened.
    pub async fn open_existing(path: &Path) -> Result<Self, HostError> {
        existing_regular_file(path)?;
        let journal = Self {
            path: path.to_path_buf(),
            read_only: false,
            protected_path: None,
        };
        let _connection = journal.connection().await?;
        Ok(journal)
    }

    /// Whether the durable host-wide admission stop is set.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the control row cannot be read.
    pub async fn draining(&self) -> Result<bool, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query("SELECT draining FROM controller_state WHERE id = 1", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?;
        match row
            .ok_or(HostError::Journal)?
            .get::<i64>(0)
            .map_err(|_| HostError::Journal)?
        {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(HostError::Journal),
        }
    }

    /// Persist host-wide drain intent. Repeated requests retain the first timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the state cannot be committed.
    pub async fn request_drain(&self) -> Result<(), HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let now = unix_millis();
        let updated = conn
            .execute(
                "UPDATE controller_state SET draining = 1, drain_requested_at_ms = COALESCE(drain_requested_at_ms, ?1) WHERE id = 1",
                [now],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if updated != 1 {
            let _rolled_back = conn.execute("ROLLBACK", ()).await;
            return Err(HostError::Journal);
        }
        conn.execute("COMMIT", ())
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    }

    /// Resume host-wide admission after an explicit operator request.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the state cannot be committed.
    pub async fn resume(&self) -> Result<(), HostError> {
        if self.read_only {
            return Err(HostError::Journal);
        }
        let conn = self.connection().await?;
        conn.execute("BEGIN IMMEDIATE", ())
            .await
            .map_err(|_| HostError::Journal)?;
        let updated = conn
            .execute(
                "UPDATE controller_state SET draining = 0, drain_requested_at_ms = NULL WHERE id = 1",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        if updated != 1 {
            let _rolled_back = conn.execute("ROLLBACK", ()).await;
            return Err(HostError::Journal);
        }
        conn.execute("COMMIT", ())
            .await
            .map_err(|_| HostError::Journal)?;
        Ok(())
    }
}

fn existing_regular_file(path: &Path) -> Result<(), HostError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| HostError::Journal)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(HostError::Journal);
    }
    Ok(())
}

fn unix_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    i64::try_from(millis).unwrap_or(i64::MAX)
}
