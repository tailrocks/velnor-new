//! Read-only controller readiness. No secret is logged and no worker is started.

use sha2::Digest;
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
const HEX: &[u8; 16] = b"0123456789abcdef";

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
    Valid {
        config: Box<HostConfig>,
        config_sha256: String,
    },
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
    F: FnMut(&KeychainReference) -> bool,
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
    let (config, config_sha256) = match read_config(state) {
        ConfigFact::Missing => return Readiness::WaitingForCredentials,
        ConfigFact::Valid {
            config,
            config_sha256,
        } => (*config, config_sha256),
        ConfigFact::Invalid => return Readiness::Degraded,
    };
    let mut has_credential = has_credential;
    let mut missing_credential = false;
    for connection in config.connections() {
        if !has_credential(&connection.credential_ref) {
            eprintln!(
                "readiness=waiting_for_credentials connection_id={}",
                connection.connection_id
            );
            missing_credential = true;
        }
    }
    match config.release_read_credential_ref() {
        Some(reference) if !has_credential(reference) => {
            eprintln!("readiness=waiting_for_credentials credential=release_read");
            missing_credential = true;
        }
        Some(_) => {}
        None => {
            eprintln!("readiness=waiting_for_credentials reason=release_read_reference_missing");
            missing_credential = true;
        }
    }
    if missing_credential {
        return Readiness::WaitingForCredentials;
    }
    if let Err(reason) = crate::launch::build_pin_status() {
        eprintln!("readiness=degraded reason=source_pin detail={reason}");
        return Readiness::Degraded;
    }
    let Some((engine_id, docker_root_digest)) = engine_identity(&config, deadline).await else {
        return Readiness::WaitingForEngine;
    };
    let journal = journal_mark(&state.join("launch.db"), deadline).await;
    match classify_engine_journal(true, journal) {
        Readiness::Reconciling => crate::controller_readiness_status(
            state,
            deadline.saturating_duration_since(tokio::time::Instant::now()),
            &config_sha256,
            &engine_id,
            &docker_root_digest,
        )
        .unwrap_or(Readiness::Reconciling),
        readiness => readiness,
    }
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
    let mut config_sha256 = String::with_capacity(64);
    for byte in sha2::Sha256::digest(&bytes) {
        config_sha256.push(char::from(HEX[usize::from(byte >> 4)]));
        config_sha256.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    match std::str::from_utf8(&bytes)
        .map_err(|_| ())
        .and_then(|text| HostConfig::parse(text).map_err(|_| ()))
    {
        Ok(config) => ConfigFact::Valid {
            config: Box::new(config),
            config_sha256,
        },
        Err(()) => ConfigFact::Invalid,
    }
}

pub(crate) fn controller_config(state: &Path) -> Option<(HostConfig, String)> {
    match read_config(state) {
        ConfigFact::Valid {
            config,
            config_sha256,
        } => Some((*config, config_sha256)),
        ConfigFact::Missing | ConfigFact::Invalid => None,
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

async fn engine_identity(
    config: &HostConfig,
    deadline: tokio::time::Instant,
) -> Option<(String, String)> {
    if tokio::time::Instant::now() >= deadline {
        return None;
    }
    let Ok(docker) = connect_unix(&config.docker.endpoint) else {
        return None;
    };
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    let budget = ENGINE_BUDGET.min(remaining);
    let info = docker_deadline_after(docker.info(), budget)
        .await
        .ok()?
        .ok()?;
    let engine_id = info.id.as_deref()?.to_owned();
    let selected = crate::journal::EngineContext {
        engine_id: engine_id.clone(),
        endpoint: config.docker.endpoint.clone(),
        platform: config.docker.platform.clone(),
    };
    let selected = crate::launch::SelectedEngineContext::capture(&selected, &info).ok()?;
    Some((engine_id, selected.docker_root_digest().to_owned()))
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
