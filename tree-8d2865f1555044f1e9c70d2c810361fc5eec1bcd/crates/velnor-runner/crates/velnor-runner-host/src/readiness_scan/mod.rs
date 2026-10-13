//! Read-only controller readiness. No secret is logged and no worker is started.

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
    runtime.block_on(assess(state, service, account))
}

async fn assess(state: &Path, service: &str, account: &str) -> Readiness {
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
    let journal = journal_mark(&state.join("launch.db")).await;
    classify(Facts {
        config_ok: Answer::Yes,
        credential: Answer::Yes,
        engine: Answer::Yes,
        journal,
        ..blank(false, true)
    })
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

async fn journal_mark(path: &Path) -> JournalFact {
    if !path.is_file() {
        return JournalFact::Clear;
    }
    match scan_rows(path).await {
        Ok(rows) if rows.iter().any(occupies) => JournalFact::Occupied,
        Ok(_) => JournalFact::Clear,
        Err(_) => JournalFact::Unreadable,
    }
}

async fn scan_rows(path: &Path) -> Result<Vec<IntentRow>, HostError> {
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
    let mut rows = Vec::new();
    while let Some(row) = query.next().await.map_err(|_| HostError::Journal)? {
        rows.push(marker_row(&row)?);
    }
    Ok(rows)
}

fn marker_row(row: &turso::Row) -> Result<IntentRow, HostError> {
    let state_text: String = row.get(1).map_err(|_| HostError::Journal)?;
    let proven: i64 = row.get(2).map_err(|_| HostError::Journal)?;
    Ok(IntentRow {
        id: 0,
        kind: row.get(0).map_err(|_| HostError::Journal)?,
        subject: String::new(),
        state: IntentState::parse(&state_text)?,
        docker_id: None,
        dind_id: None,
        worker_volume: None,
        github_runner_id: None,
        cleanup_proven: proven != 0,
    })
}

#[cfg(test)]
mod tests;
