//! Read-only controller readiness. No secret is logged and no worker is started.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use crate::config::HostConfig;
use crate::docker_client::{connect_unix, docker_deadline_after};
use crate::error::HostError;
use crate::journal::IntentState;
use crate::keychain::load_secret;
use crate::readiness::{Readiness, readiness_for_empty};
use crate::reconcile::{IntentRow, occupies};

const ENGINE_BUDGET: Duration = Duration::from_secs(2);
const READINESS_BUDGET: Duration = Duration::from_secs(4);

/// One readiness input. A bool would trip the struct flag limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// The check succeeded.
    Yes,
    /// The check failed or was not run.
    No,
}

/// Inputs for [`classify`]. Each flag stays independent so tests can force a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Facts {
    /// `state/drain` exists.
    drain: Answer,
    /// `state` is a directory.
    directory: Answer,
    /// `host.toml` parsed.
    config_ok: Answer,
    /// Keychain item is non-empty UTF-8. The bytes are not retained here.
    credential: Answer,
    /// Engine socket answered a bounded ping.
    engine: Answer,
    /// Journal occupancy. Absent journals are [`JournalFact::Clear`].
    journal: JournalFact,
}

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

/// Report drain, credentials, engine, and journal occupancy.
///
/// A missing directory is left untouched. The journal is opened read-only and
/// is not created.
#[must_use]
pub fn controller_readiness(state: &Path, service: &str, account: &str) -> Readiness {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build();
    let Ok(runtime) = runtime else {
        return Readiness::Degraded;
    };
    let deadline = tokio::time::Instant::now() + READINESS_BUDGET;
    runtime.block_on(bounded_readiness(
        assess(state, service, account, deadline),
        deadline,
    ))
}

async fn assess(
    state: &Path,
    service: &str,
    account: &str,
    deadline: tokio::time::Instant,
) -> Readiness {
    let drain = state.join("drain").is_file();
    if drain || !state.is_dir() {
        return classify(blank(drain, state.is_dir()));
    }
    let Some(config) = read_config(state) else {
        return classify(blank(false, true));
    };
    if !credential_present(service, account) {
        return classify(Facts {
            config_ok: Answer::Yes,
            ..blank(false, true)
        });
    }
    if !engine_up(&config.docker.endpoint).await {
        return classify(Facts {
            config_ok: Answer::Yes,
            credential: Answer::Yes,
            ..blank(false, true)
        });
    }
    let journal = journal_mark(&state.join("launch.db"), deadline).await;
    classify(Facts {
        config_ok: Answer::Yes,
        credential: Answer::Yes,
        engine: Answer::Yes,
        journal,
        ..blank(false, true)
    })
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

fn classify(facts: Facts) -> Readiness {
    if facts.drain == Answer::Yes {
        return Readiness::Draining;
    }
    let blocked = facts.directory == Answer::No
        || facts.config_ok == Answer::No
        || facts.credential == Answer::No;
    if blocked {
        return readiness_for_empty();
    }
    if facts.engine == Answer::No {
        return Readiness::WaitingForEngine;
    }
    match facts.journal {
        JournalFact::Unreadable => Readiness::Degraded,
        JournalFact::Occupied => Readiness::Reconciling,
        JournalFact::Clear => Readiness::Ready,
    }
}

fn blank(drain: bool, directory: bool) -> Facts {
    Facts {
        drain: answer(drain),
        directory: answer(directory),
        config_ok: Answer::No,
        credential: Answer::No,
        engine: Answer::No,
        journal: JournalFact::Clear,
    }
}

fn answer(value: bool) -> Answer {
    if value { Answer::Yes } else { Answer::No }
}

fn read_config(state: &Path) -> Option<HostConfig> {
    let text = std::fs::read_to_string(state.join("host.toml")).ok()?;
    HostConfig::parse(&text).ok()
}

fn credential_present(service: &str, account: &str) -> bool {
    let Ok(secret) = load_secret(service, account) else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(secret.as_slice()) else {
        return false;
    };
    !text.trim().is_empty()
}

async fn engine_up(endpoint: &str) -> bool {
    let Ok(docker) = connect_unix(endpoint) else {
        return false;
    };
    matches!(
        docker_deadline_after(docker.ping(), ENGINE_BUDGET).await,
        Ok(Ok(_))
    )
}

async fn journal_mark(path: &Path, deadline: tokio::time::Instant) -> JournalFact {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return JournalFact::Clear,
        Ok(_) | Err(_) => return JournalFact::Unreadable,
    }
    bounded_journal_scan(scan_rows(path), deadline).await
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

async fn scan_rows(path: &Path) -> Result<bool, HostError> {
    let text = path.to_str().ok_or(HostError::Path)?;
    let db = turso::Builder::new_local(text)
        .read_only(true)
        .build()
        .await
        .map_err(|_| HostError::Journal)?;
    let conn = db.connect().map_err(|_| HostError::Journal)?;
    let mut query = conn
        .query("SELECT kind, state, cleanup_proven FROM intents", ())
        .await
        .map_err(|_| HostError::Journal)?;
    while let Some(row) = query.next().await.map_err(|_| HostError::Journal)? {
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
        github_runner_id: None,
        cleanup_proven,
    })
}

#[cfg(test)]
mod tests;
