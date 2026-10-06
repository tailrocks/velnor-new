//! Process-shared journal locks and external lineage ownership.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::error::HostError;

use super::Journal;

#[derive(Debug)]
pub(super) struct JournalProcessState {
    pub(super) lineage_guard: Mutex<Option<crate::daemon_lock::EngineLineageGuard>>,
    pub(super) write_lock: tokio::sync::Mutex<()>,
    completion_locks: Mutex<HashMap<i64, Weak<tokio::sync::Mutex<()>>>>,
}

pub(super) fn process_state(path: &Path) -> Result<Arc<JournalProcessState>, HostError> {
    static STATES: OnceLock<Mutex<HashMap<PathBuf, Weak<JournalProcessState>>>> = OnceLock::new();
    let states = STATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut states = states.lock().map_err(|_| HostError::Lock)?;
    states.retain(|_, state| state.strong_count() > 0);
    if let Some(state) = states.get(path).and_then(Weak::upgrade) {
        return Ok(state);
    }
    let state = Arc::new(JournalProcessState {
        lineage_guard: Mutex::new(None),
        write_lock: tokio::sync::Mutex::new(()),
        completion_locks: Mutex::new(HashMap::new()),
    });
    states.insert(path.to_path_buf(), Arc::downgrade(&state));
    Ok(state)
}

impl JournalProcessState {
    fn completion_lock(&self, id: i64) -> Result<Arc<tokio::sync::Mutex<()>>, HostError> {
        if id <= 0 {
            return Err(HostError::Journal);
        }
        let mut locks = self.completion_locks.lock().map_err(|_| HostError::Lock)?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&id).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(id, Arc::downgrade(&lock));
        Ok(lock)
    }
}

impl Journal {
    /// Attach the process-held engine lineage guard after startup validation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Lock`] when the shared guard slot is poisoned.
    pub(crate) fn attach_lineage_guard(
        &self,
        guard: crate::daemon_lock::EngineLineageGuard,
    ) -> Result<(), HostError> {
        let mut current = self
            .process
            .lineage_guard
            .lock()
            .map_err(|_| HostError::Lock)?;
        if current.is_none() {
            *current = Some(guard);
        }
        Ok(())
    }

    pub(crate) async fn sync_lineage(&self) -> Result<(), HostError> {
        let guard = self
            .process
            .lineage_guard
            .lock()
            .map_err(|_| HostError::Lock)?
            .clone();
        let Some(guard) = guard else {
            return Ok(());
        };
        guard.advance_revision(self.revision().await?)
    }

    pub(crate) async fn write_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.process.write_lock.lock().await
    }

    pub(crate) fn completion_lock(
        &self,
        id: i64,
    ) -> Result<Arc<tokio::sync::Mutex<()>>, HostError> {
        self.process.completion_lock(id)
    }

    pub(crate) async fn sync_after<T>(&self, result: Result<T, HostError>) -> Result<T, HostError> {
        self.sync_lineage().await?;
        result
    }
}
