//! Clear an empty launch whose runner name is already registered.
//!
//! An uncertain row with no container id holds one slot. JIT then returns
//! HTTP 409 and the same message is delivered again. This module removes only
//! an offline idle registration, fails that empty row, and lets mint retry.
//! A busy or online runner stays. A list or delete error is not absence.

use std::future::Future;

use serde::Deserialize;
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};

use crate::error::HostError;
use crate::journal::{IntentState, Journal, Outcome};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::steps;
use super::{Drive, Lane, bind};

/// One directory answer. `Unknown` does not free the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// The name is free, or the offline id was deleted.
    Free,
    /// Online or busy. Do not delete.
    Live,
    /// The directory did not answer. Do not fail the row.
    Unknown,
    /// This offline id was already removed for this subject. Do not delete it.
    Repeat,
}

/// Mint `name` once. One empty uncertain row may be failed and retried once.
///
/// # Errors
///
/// Returns [`EnsureError::Uncertain`] when the directory or the row is not clear.
/// A cleared name is not [`EnsureError::Conflict`]. The caller must not acknowledge it.
pub(super) async fn ensure_runner<T, S, F>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    name: &str,
    subject: &str,
    batch: Option<&velnor_runner_github::ParsedBatch>,
    start: S,
) -> Result<Option<Started>, EnsureError>
where
    T: Transport + Lane,
    S: Fn(&str, &[u8], bind::Bind) -> F,
    F: Future<Output = Result<Started, HostError>>,
{
    lane.on_admin()?;
    let mut extra = 1u8;
    loop {
        let (id, fresh) = journal
            .begin_launch(subject)
            .await
            .map_err(steps::map_journal)?;
        if steps::docker_of(journal, id).await?.is_some() {
            return steps::finish_live(lane, ctx, journal, id, batch).await;
        }
        if !fresh {
            match release_empty(lane, ctx, journal, id, name).await? {
                // The cleared row was not a mint. The next mint keeps its one retry.
                Decision::Free if extra > 0 => continue,
                // The row is already failed. A repeat must not hold the slot.
                Decision::Free | Decision::Repeat => return Err(EnsureError::NameSteady),
                _ => return steps::hold(journal, id, EnsureError::Uncertain).await,
            }
        }
        match steps::mint(lane, ctx, batch, journal, id, name, &start).await {
            Err(EnsureError::NameCleared) if extra > 0 => {
                extra -= 1;
            }
            // The delete already ran. Another poll must not delete that id again.
            Err(EnsureError::NameCleared) => return Err(EnsureError::NameSteady),
            other => return other,
        }
    }
}

/// Classify JIT HTTP 409. Do not acknowledge a name that was just removed.
///
/// # Errors
///
/// Returns [`EnsureError`] when the journal write fails or the admin origin is bad.
pub(super) async fn after_jit<T>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    id: i64,
    name: &str,
    mapped: EnsureError,
) -> Result<Option<Started>, EnsureError>
where
    T: Transport + Lane,
{
    if !matches!(mapped, EnsureError::Conflict) {
        return steps::hold(journal, id, mapped).await;
    }
    if !credentials(ctx) || !row_is_empty(journal, id).await? {
        return steps::hold(journal, id, EnsureError::Uncertain).await;
    }
    match decide(lane, ctx, journal, id, name).await? {
        Decision::Free | Decision::Repeat => {
            journal
                .finish(id, Outcome::DefiniteFailure)
                .await
                .map_err(steps::map_journal)?;
            Err(EnsureError::NameCleared)
        }
        Decision::Live => {
            journal
                .finish(id, Outcome::DefiniteFailure)
                .await
                .map_err(steps::map_journal)?;
            Err(EnsureError::Conflict)
        }
        Decision::Unknown => steps::hold(journal, id, EnsureError::Uncertain).await,
    }
}

async fn release_empty<T>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    id: i64,
    name: &str,
) -> Result<Decision, EnsureError>
where
    T: Transport + Lane,
{
    if !credentials(ctx) || !row_is_empty(journal, id).await? {
        return Ok(Decision::Unknown);
    }
    match decide(lane, ctx, journal, id, name).await? {
        Decision::Free => {
            journal
                .finish(id, Outcome::DefiniteFailure)
                .await
                .map_err(steps::map_journal)?;
            Ok(Decision::Free)
        }
        Decision::Repeat => {
            journal
                .finish(id, Outcome::DefiniteFailure)
                .await
                .map_err(steps::map_journal)?;
            Ok(Decision::Repeat)
        }
        other => Ok(other),
    }
}

fn credentials(ctx: &Drive) -> bool {
    runners_path(&ctx.owner, &ctx.repo).is_some() && !ctx.pat.is_empty()
}

async fn row_is_empty(journal: &Journal, id: i64) -> Result<bool, EnsureError> {
    let rows = journal.rows().await.map_err(steps::map_journal)?;
    let Some(row) = rows.into_iter().find(|row| row.id == id) else {
        return Ok(false);
    };
    if row.state == IntentState::Done || row.docker_id.is_some() || row.dind_id.is_some() {
        return Ok(false);
    }
    Ok(row.worker_volume.as_deref().is_none_or(str::is_empty))
}

async fn decide<T>(
    lane: &mut T,
    ctx: &Drive,
    journal: &Journal,
    row: i64,
    name: &str,
) -> Result<Decision, EnsureError>
where
    T: Transport + Lane,
{
    let Some(path) = runners_path(&ctx.owner, &ctx.repo) else {
        return Ok(Decision::Unknown);
    };
    let listed = match github(lane, &list_request(&path, &ctx.pat)?)? {
        Ok(exchange) if exchange.status == 200 => classify_list(&exchange.body, name),
        _ => return Ok(Decision::Unknown),
    };
    let Listed::Offline(id) = listed else {
        return Ok(listed_decision(listed));
    };
    if recorded(journal, name, id).await? {
        return Ok(Decision::Repeat);
    }
    let decision = delete_offline(lane, &path, id, &ctx.pat)?;
    if decision == Decision::Free {
        remember(journal, row, id).await?;
    }
    Ok(decision)
}

async fn recorded(journal: &Journal, name: &str, id: i64) -> Result<bool, EnsureError> {
    let needle = id.to_string();
    let rows = journal.rows().await.map_err(steps::map_journal)?;
    Ok(rows.iter().any(|row| {
        row.kind == "launch"
            && row.subject == name
            && row.github_runner_id.as_deref() == Some(needle.as_str())
    }))
}

async fn remember(journal: &Journal, row: i64, id: i64) -> Result<(), EnsureError> {
    let text = id.to_string();
    journal
        .bind(row, None, Some(&text))
        .await
        .map_err(steps::map_journal)
}

const fn listed_decision(listed: Listed) -> Decision {
    match listed {
        Listed::Absent => Decision::Free,
        // `decide` deletes `Offline` before this arm. Do not treat it as free here.
        Listed::Live | Listed::Offline(_) => Decision::Live,
        Listed::Unknown => Decision::Unknown,
    }
}

fn delete_offline<T>(lane: &mut T, path: &str, id: i64, pat: &str) -> Result<Decision, EnsureError>
where
    T: Transport + Lane,
{
    let delete_path = format!("{path}/{id}");
    match github(lane, &delete_request(&delete_path, pat)?)? {
        Ok(exchange) if exchange.status == 204 || exchange.status == 404 => Ok(Decision::Free),
        _ => Ok(Decision::Unknown),
    }
}

fn runners_path(owner: &str, repo: &str) -> Option<String> {
    if owner.is_empty()
        || repo.is_empty()
        || owner.contains('/')
        || repo.contains('/')
        || owner.contains(' ')
        || repo.contains(' ')
    {
        return None;
    }
    Some(format!("/repos/{owner}/{repo}/actions/runners"))
}

fn list_request(path: &str, pat: &str) -> Result<SessionRequest, EnsureError> {
    Ok(SessionRequest {
        method: Method::Get,
        path: path.to_owned(),
        query: Some("per_page=100".to_owned()),
        headers: vec![accept(), bearer(pat)?],
        body: Vec::new(),
    })
}

fn delete_request(path: &str, pat: &str) -> Result<SessionRequest, EnsureError> {
    Ok(SessionRequest {
        method: Method::Delete,
        path: path.to_owned(),
        query: None,
        headers: vec![accept(), bearer(pat)?],
        body: Vec::new(),
    })
}

fn accept() -> (String, String) {
    (
        "Accept".to_owned(),
        "application/vnd.github+json".to_owned(),
    )
}

fn bearer(pat: &str) -> Result<(String, String), EnsureError> {
    if pat.is_empty() {
        return Err(EnsureError::Rejected);
    }
    Ok(("Authorization".to_owned(), format!("Bearer {pat}")))
}

fn github<T>(
    lane: &mut T,
    request: &SessionRequest,
) -> Result<Result<Exchange, TransportFail>, EnsureError>
where
    T: Transport + Lane,
{
    lane.use_github_api()?;
    let exchange = lane.exchange(request);
    lane.on_admin()?;
    Ok(exchange)
}

#[derive(Deserialize)]
struct Page {
    total_count: usize,
    runners: Vec<One>,
}

#[derive(Deserialize)]
struct One {
    id: i64,
    name: String,
    status: String,
    busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listed {
    Absent,
    Offline(i64),
    Live,
    Unknown,
}

fn classify_list(body: &[u8], name: &str) -> Listed {
    let Ok(page) = serde_json::from_slice::<Page>(body) else {
        return Listed::Unknown;
    };
    if let Some(row) = page.runners.iter().find(|row| row.name == name) {
        if row.busy || row.status != "offline" || row.id <= 0 {
            return Listed::Live;
        }
        return Listed::Offline(row.id);
    }
    if page.runners.len() < page.total_count {
        return Listed::Unknown;
    }
    Listed::Absent
}

#[cfg(test)]
mod tests;
