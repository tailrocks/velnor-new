//! Read-only inspection and durable controller admission state.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::HostError;

use super::{DrainSnapshot, Journal};

impl Journal {
    /// Open an existing journal without running schema bootstrap or permitting writes.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the path is missing, not a regular file,
    /// a symlink, or cannot be opened read-only, or
    /// [`HostError::UnsupportedJournalVersion`] for a future schema.
    pub async fn open_readonly(path: &Path) -> Result<Self, HostError> {
        existing_regular_file(path)?;
        let journal = Self {
            path: path.to_path_buf(),
            read_only: true,
            protected_path: None,
        };
        validate_existing_version(&journal).await?;
        Ok(journal)
    }

    /// Open an existing journal read-only beneath the exact retained host
    /// state-directory identity. This does not create a database, bootstrap or
    /// migrate the schema, or create sidecars.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Path`] when the parent, database, or sidecars are
    /// unsafe or no longer match the retained identity, [`HostError::Journal`]
    /// when the existing journal cannot be opened read-only, or
    /// [`HostError::UnsupportedJournalVersion`] for a future schema.
    pub async fn open_readonly_protected_at(
        path: &Path,
        parent_device: u64,
        parent_inode: u64,
    ) -> Result<Self, HostError> {
        let protected_path =
            super::protected_path::ProtectedJournalPath::inspect_existing_for_parent(
                path,
                (parent_device, parent_inode),
            )?;
        let journal = Self {
            path: path.to_path_buf(),
            read_only: true,
            protected_path: Some(protected_path),
        };
        validate_existing_version(&journal).await?;
        Ok(journal)
    }

    /// Open an existing journal for controller mutations under the exact
    /// retained parent identity, without schema bootstrap or migration.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Path`] when the parent, database, or sidecars are
    /// unsafe or no longer match the retained identity, and
    /// [`HostError::Journal`] when the existing journal cannot be opened, or
    /// [`HostError::UnsupportedJournalVersion`] for a future schema.
    pub async fn open_existing_protected_at(
        path: &Path,
        parent_device: u64,
        parent_inode: u64,
    ) -> Result<Self, HostError> {
        let protected_path =
            super::protected_path::ProtectedJournalPath::inspect_existing_for_parent(
                path,
                (parent_device, parent_inode),
            )?;
        let journal = Self {
            path: path.to_path_buf(),
            read_only: false,
            protected_path: Some(protected_path),
        };
        validate_existing_version(&journal).await?;
        Ok(journal)
    }

    /// Open an existing journal for state changes without creating or migrating it.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the path is missing, not a regular file,
    /// a symlink, or cannot be opened, or
    /// [`HostError::UnsupportedJournalVersion`] for a future schema.
    pub async fn open_existing(path: &Path) -> Result<Self, HostError> {
        existing_regular_file(path)?;
        let journal = Self {
            path: path.to_path_buf(),
            read_only: false,
            protected_path: None,
        };
        validate_existing_version(&journal).await?;
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

    /// Read the drain fence and bounded aggregate occupancy in one SQL snapshot.
    ///
    /// Occupancy follows `velnor_runner_launch_slot::holds`; unresolved intent
    /// counting follows the Linux shutdown summary's journal predicate. This is
    /// a local journal view only and does not prove Docker ownership is empty.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for an absent or malformed controller row,
    /// corrupt counts, or database failure.
    pub async fn drain_snapshot(&self) -> Result<DrainSnapshot, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT (SELECT draining FROM controller_state WHERE id = 1), (SELECT COUNT(*) FROM intents WHERE kind = 'launch' AND cleanup_proven = 0 AND NOT (state = 'failed' AND effect_state = 'definite_no_effect')), (SELECT COUNT(*) FROM intents WHERE kind != 'launch' AND CASE WHEN cleanup_proven = 1 THEN 0 WHEN kind = 'discovery-credential' THEN state IN ('pending', 'uncertain') WHEN kind = 'scale-set-session' THEN NOT (state = 'done' AND EXISTS (SELECT 1 FROM scale_set_sessions WHERE intent_id = intents.id AND state = 'closed')) AND NOT (state = 'failed' AND effect_state = 'definite_no_effect') WHEN state = 'failed' AND effect_state = 'definite_no_effect' THEN 0 ELSE 1 END = 1)",
                (),
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows.next().await.map_err(|_| HostError::Journal)?;
        let row = row.ok_or(HostError::Journal)?;
        let draining = match row.get::<i64>(0).map_err(|_| HostError::Journal)? {
            0 => false,
            1 => true,
            _ => return Err(HostError::Journal),
        };
        let occupied_launches = nonnegative_count(row.get(1).map_err(|_| HostError::Journal)?)?;
        let unresolved_intents = nonnegative_count(row.get(2).map_err(|_| HostError::Journal)?)?;
        Ok(DrainSnapshot {
            draining,
            occupied_launches,
            unresolved_intents,
        })
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

async fn validate_existing_version(journal: &Journal) -> Result<(), HostError> {
    let conn = journal.connection().await?;
    super::schema::validate_existing_version(&conn).await
}

fn nonnegative_count(value: i64) -> Result<u64, HostError> {
    u64::try_from(value).map_err(|_| HostError::Journal)
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
