//! Immutable engine, launch, and resource identities in the journal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::daemon_lock::EngineLineageGuard;
use crate::error::HostError;
use crate::journal::Journal;
use crate::journal_schema;
use crate::journal_sql::{one_row, token_rejected};
use crate::launch_identity::LaunchIdentity;

#[derive(Debug)]
struct JournalProcessState {
    lineage_guard: Mutex<Option<EngineLineageGuard>>,
    write_lock: Arc<tokio::sync::Mutex<()>>,
}

static STATES: OnceLock<Mutex<HashMap<PathBuf, Arc<JournalProcessState>>>> = OnceLock::new();

fn process_state(path: &Path) -> Result<Arc<JournalProcessState>, HostError> {
    let states = STATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut states = states.lock().map_err(|_| HostError::Lock)?;
    if let Some(state) = states.get(path) {
        return Ok(Arc::clone(state));
    }
    let state = Arc::new(JournalProcessState {
        lineage_guard: Mutex::new(None),
        write_lock: Arc::new(tokio::sync::Mutex::new(())),
    });
    states.insert(path.to_path_buf(), Arc::clone(&state));
    Ok(state)
}

fn release_process_state(path: &Path) {
    if let Some(states) = STATES.get()
        && let Ok(mut states) = states.lock()
    {
        states.remove(path);
    }
}

impl Drop for Journal {
    fn drop(&mut self) {
        release_process_state(self.path());
    }
}

impl Journal {
    /// Attach the process-held engine lineage guard after startup validation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Lock`] when the shared guard slot is poisoned.
    pub(crate) fn attach_lineage_guard(&self, guard: EngineLineageGuard) -> Result<(), HostError> {
        let state = process_state(self.path())?;
        let mut current = state.lineage_guard.lock().map_err(|_| HostError::Lock)?;
        if current.is_none() {
            *current = Some(guard);
        }
        Ok(())
    }

    pub(crate) async fn sync_lineage(&self) -> Result<(), HostError> {
        let state = process_state(self.path())?;
        let guard = state
            .lineage_guard
            .lock()
            .map_err(|_| HostError::Lock)?
            .clone();
        let Some(guard) = guard else {
            return Ok(());
        };
        guard.advance_revision(self.revision().await?)
    }

    pub(crate) async fn write_guard(&self) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = process_state(self.path())
            .map(|state| Arc::clone(&state.write_lock))
            .unwrap_or_default();
        lock.lock_owned().await
    }

    pub(crate) async fn sync_after<T>(&self, result: Result<T, HostError>) -> Result<T, HostError> {
        self.sync_lineage().await?;
        result
    }
}

impl Journal {
    /// Bind this journal to one Docker engine. A changed engine fails closed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the id is invalid or changes.
    pub async fn bind_engine(&self, engine_id: &str) -> Result<(), HostError> {
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let result = journal_schema::bind_engine(&conn, engine_id).await;
        self.sync_after(result).await
    }

    /// Read the journal's stable instance identity.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the identity is absent.
    pub(crate) async fn instance_id(&self) -> Result<String, HostError> {
        let conn = self.connection().await?;
        journal_schema::instance_id(&conn).await
    }

    /// Read the monotonic state revision committed with journal writes.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when the revision is absent or invalid.
    pub(crate) async fn revision(&self) -> Result<u64, HostError> {
        let conn = self.connection().await?;
        journal_schema::revision(&conn).await
    }

    /// Read one launch identity after the engine is bound.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] for a legacy row or an unbound engine.
    pub async fn launch_identity(&self, id: i64) -> Result<LaunchIdentity, HostError> {
        let conn = self.connection().await?;
        let mut rows = conn
            .query(
                "SELECT launch_id FROM intents WHERE id = ?1 AND kind = 'launch'",
                [id],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let launch_id: Option<String> = row.get(0).map_err(|_| HostError::Journal)?;
        let launch_id = launch_id.ok_or(HostError::Journal)?;
        let instance_id = journal_schema::instance_id(&conn).await?;
        let engine_id = journal_schema::engine_id(&conn).await?;
        LaunchIdentity::new(&instance_id, id, &launch_id, &engine_id)
    }

    /// Bind both immutable Docker ids. Rebinding to another id fails.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when either id conflicts or the row is missing.
    pub async fn bind_pair(
        &self,
        id: i64,
        runner_id: &str,
        dind_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(runner_id) || token_rejected(dind_id) {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET docker_id = COALESCE(docker_id, ?1), dind_id = COALESCE(dind_id, ?2) WHERE id = ?3 AND kind = 'launch' AND cleanup_proven = 0 AND (docker_id IS NULL OR docker_id = ?1) AND (dind_id IS NULL OR dind_id = ?2)",
                (runner_id, dind_id, id),
            )
            .await
            .map_err(|_| HostError::Journal);
        self.sync_after(changed.and_then(one_row)).await
    }

    /// Bind one immutable seed generation to a launch.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Journal`] when a different generation is already bound.
    pub async fn bind_seed_generation(
        &self,
        id: i64,
        generation_id: &str,
    ) -> Result<(), HostError> {
        if token_rejected(generation_id) {
            return Err(HostError::Journal);
        }
        let _write = self.write_guard().await;
        self.sync_lineage().await?;
        let conn = self.connection().await?;
        let changed = conn
            .execute(
                "UPDATE intents SET seed_generation_id = COALESCE(seed_generation_id, ?1) WHERE id = ?2 AND kind = 'launch' AND cleanup_proven = 0 AND (seed_generation_id IS NULL OR seed_generation_id = ?1)",
                (generation_id, id),
            )
            .await
            .map_err(|_| HostError::Journal);
        self.sync_after(changed.and_then(one_row)).await
    }
}
