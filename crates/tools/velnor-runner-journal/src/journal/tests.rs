//! Journal durability. Each call commits before it returns.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{HostError, IntentRow, IntentState, Journal};

pub(super) struct Scratch {
    pub(super) path: PathBuf,
}

impl Scratch {
    pub(super) fn new(label: &str) -> Result<Self, HostError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("velnor-host-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).map_err(|_| HostError::Journal)?;
        Ok(Self { path })
    }

    pub(super) fn file(&self) -> PathBuf {
        self.path.join("journal.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let cleanup = std::fs::remove_dir_all(&self.path);
        let _kept = cleanup.err().map(|err| err.kind());
    }
}

pub(super) async fn open(path: &Path) -> Result<Journal, HostError> {
    Journal::open(path).await
}

pub(super) struct Seen {
    pub(super) state: Option<IntentState>,
    pub(super) secret_ok: bool,
}

pub(super) async fn observed_state(path: &Path) -> Option<IntentState> {
    let other = Journal::open(path).await.ok()?;
    let rows = other.rows().await.ok()?;
    rows.first().map(|row| row.state)
}

pub(super) fn dir_contains(dir: &Path, needle: &str) -> Result<bool, HostError> {
    let raw = needle.as_bytes();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current).map_err(|_| HostError::Journal)?;
        for entry in entries {
            let path = entry.map_err(|_| HostError::Journal)?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).map_err(|_| HostError::Journal)?;
            if bytes.windows(raw.len()).any(|window| window == raw) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub(super) fn intent(state: IntentState, kind: &str) -> IntentRow {
    IntentRow {
        id: 1,
        kind: kind.to_owned(),
        subject: "job".to_owned(),
        state,
        launch_effect: crate::journal::LaunchEffectState::Unknown,
        docker_id: None,
        dind_id: None,
        worker_volume: None,
        github_runner_id: None,
        message_id: None,
        runner_request_id: None,
        requested_workflow_run_id: None,
        requested_job_id: None,
        runner_name: None,
        observed_job_id: None,
        observed_workflow_run_id: None,
        remote_terminal: false,
        cleanup_proven: false,
        outer_network_name: None,
        outer_network_id: None,
        runner_start_intent: crate::journal::RunnerStartIntent::UnknownLegacy,
    }
}

mod cleanup_tests;
mod durability_tests;
mod lifecycle_tests;
mod reconcile_tests;
mod release_tests;
mod runner_event_tests;
