//! Read-only controller readiness. No secret is logged and no worker is started.

use std::future::Future;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;

use crate::config::{HostConfig, KeychainReference};
use crate::docker_client::{connect_unix, docker_deadline_after};
use crate::error::HostError;
use crate::journal::IntentState;
use crate::keychain::load_secret;
use crate::readiness::{Readiness, readiness_for_empty};
use crate::reconcile::{IntentRow, occupies};

const ENGINE_BUDGET: Duration = Duration::from_secs(2);
/// Absolute wall-clock budget for one status or doctor assessment.
pub const READINESS_BUDGET: Duration = Duration::from_secs(4);
const MAX_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_JOURNAL_ROWS: usize = 4096;

/// Whether recorded intents still occupy a permit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JournalFact {
    /// No occupying row. A missing journal is this case.
    Clear,
    /// At least one row still occupies a permit.
    Occupied,
    /// The journal file could not be read, or a state token was unknown.
    Unreadable,
}

enum ConfigFact {
    Missing,
    Valid(Box<HostConfig>),
    Invalid,
}

/// Check the local controller state and required Docker/journal facts.
///
/// The CLI must call this in its private bounded child process because
/// synchronous filesystem and Keychain calls cannot be cooperatively stopped.
/// This partial observer never reports `Ready` without full Docker/GitHub
/// reconciliation proof.
#[must_use]
pub fn controller_readiness(state: &Path) -> Readiness {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build();
    let Ok(runtime) = runtime else {
        return Readiness::Degraded;
    };
    let deadline = tokio::time::Instant::now() + READINESS_BUDGET;
    runtime.block_on(bounded_readiness(assess(state, deadline), deadline))
}

async fn assess(state: &Path, deadline: tokio::time::Instant) -> Readiness {
    assess_with(state, deadline, credential_present).await
}

async fn assess_with<F>(
    state: &Path,
    deadline: tokio::time::Instant,
    has_credential: F,
) -> Readiness
where
    F: FnOnce(&KeychainReference) -> bool,
{
    match std::fs::symlink_metadata(state) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return readiness_for_empty();
        }
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) | Err(_) => return Readiness::Degraded,
    }
    match std::fs::symlink_metadata(state.join("drain")) {
        Ok(_) => return Readiness::Draining,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Readiness::Degraded,
    }
    let config = match read_config(state) {
        ConfigFact::Missing => return Readiness::WaitingForCredentials,
        ConfigFact::Valid(config) => *config,
        ConfigFact::Invalid => return Readiness::Degraded,
    };
    if !has_credential(&config.github.credential_ref) {
        return Readiness::WaitingForCredentials;
    }
    if !engine_up(&config.docker.endpoint, deadline).await {
        return Readiness::WaitingForEngine;
    }
    let journal = journal_mark(&state.join("launch.db"), deadline).await;
    classify_engine_journal(true, journal)
}

fn read_config(state: &Path) -> ConfigFact {
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(state.join("host.toml"))
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ConfigFact::Missing;
        }
        Err(_) => return ConfigFact::Invalid,
    };
    let Ok(metadata) = file.metadata() else {
        return ConfigFact::Invalid;
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return ConfigFact::Invalid;
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || u64::try_from(bytes.len()).map_or(true, |size| size > MAX_CONFIG_BYTES)
    {
        return ConfigFact::Invalid;
    }
    match std::str::from_utf8(&bytes)
        .map_err(|_| ())
        .and_then(|text| HostConfig::parse(text).map_err(|_| ()))
    {
        Ok(config) => ConfigFact::Valid(Box::new(config)),
        Err(()) => ConfigFact::Invalid,
    }
}

fn credential_present(reference: &KeychainReference) -> bool {
    let Ok(secret) = load_secret(reference.service(), reference.account()) else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(secret.as_slice()) else {
        return false;
    };
    !text.trim().is_empty()
}

fn classify_engine_journal(engine_up: bool, journal: JournalFact) -> Readiness {
    if !engine_up {
        return Readiness::WaitingForEngine;
    }
    match journal {
        JournalFact::Unreadable => Readiness::Degraded,
        JournalFact::Occupied | JournalFact::Clear => Readiness::Reconciling,
    }
}

async fn bounded_readiness<F>(future: F, deadline: tokio::time::Instant) -> Readiness
where
    F: Future<Output = Readiness>,
{
    match tokio::time::timeout_at(deadline, future).await {
        Ok(readiness) => readiness,
        Err(_) => Readiness::Degraded,
    }
}

async fn engine_up(endpoint: &str, deadline: tokio::time::Instant) -> bool {
    if tokio::time::Instant::now() >= deadline {
        return false;
    }
    let Ok(docker) = connect_unix(endpoint) else {
        return false;
    };
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    let budget = ENGINE_BUDGET.min(remaining);
    matches!(
        docker_deadline_after(docker.ping(), budget).await,
        Ok(Ok(_))
    )
}

async fn journal_mark(path: &Path, deadline: tokio::time::Instant) -> JournalFact {
    match std::fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_file()
                && metadata.len() <= MAX_JOURNAL_BYTES
                && tokio::time::Instant::now() < deadline => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return JournalFact::Clear,
        Ok(_) | Err(_) => return JournalFact::Unreadable,
    }
    bounded_journal_scan(scan_rows(path, deadline), deadline).await
}

async fn bounded_journal_scan<F>(future: F, deadline: tokio::time::Instant) -> JournalFact
where
    F: Future<Output = Result<bool, HostError>>,
{
    match tokio::time::timeout_at(deadline, future).await {
        Ok(Ok(true)) => JournalFact::Occupied,
        Ok(Ok(false)) => JournalFact::Clear,
        Ok(Err(_)) | Err(_) => JournalFact::Unreadable,
    }
}

async fn scan_rows(path: &Path, deadline: tokio::time::Instant) -> Result<bool, HostError> {
    let text = path.to_str().ok_or(HostError::Path)?;
    let db = turso::Builder::new_local(text)
        .read_only(true)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let conn = db.connect().map_err(|_| HostError::Journal)?;
    let mut query = conn
        .query(
            "SELECT kind, state, cleanup_proven FROM intents ORDER BY id",
            (),
        )
        .await
        .map_err(|_| HostError::Journal)?;
    let mut row_count = 0_usize;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(HostError::Journal);
        }
        let Some(row) = tokio::time::timeout_at(deadline, query.next())
            .await
            .map_err(|_| HostError::Journal)?
            .map_err(|_| HostError::Journal)?
        else {
            break;
        };
        if row_count >= MAX_JOURNAL_ROWS {
            return Err(HostError::Journal);
        }
        row_count += 1;
        if occupies(&marker_row(&row)?) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn marker_row(row: &turso::Row) -> Result<IntentRow, HostError> {
    let kind: String = row.get(0).map_err(|_| HostError::Journal)?;
    if !matches!(
        kind.as_str(),
        "acquire" | "delete" | "launch" | "provision" | "session"
    ) {
        return Err(HostError::Journal);
    }
    let state_text: String = row.get(1).map_err(|_| HostError::Journal)?;
    let raw_cleanup_proven: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    let cleanup_proven = match raw_cleanup_proven {
        0 => false,
        1 => true,
        _ => return Err(HostError::Journal),
    };
    Ok(IntentRow {
        id: 0,
        kind,
        subject: String::new(),
        state: IntentState::parse(&state_text)?,
        docker_id: None,
        dind_id: None,
        worker_volume: None,
        scale_set_id: None,
        request_id: None,
        runner_name: None,
        docker_engine_id: None,
        launch_phase: None,
        github_runner_id: None,
        cleanup_proven,
        launch_id: None,
        assignment_key: None,
        seed_generation_id: None,
        acquire_attempted: false,
        acquire_resolved: false,
        acquired: false,
        jit_requested: false,
        runner_completed: false,
    })
}

#[cfg(test)]
mod tests;
