//! Clear an empty launch whose runner name is already registered.
//!
//! An uncertain row with no container id holds one slot. JIT then returns
//! HTTP 409 and the same message is delivered again. This module removes only
//! an offline idle registration, fails that empty row, and lets mint retry.
//! A busy or online runner stays. A list or delete error is not absence.

use serde::Deserialize;
use velnor_runner_github::{
    Exchange, Method, SessionError, SessionRequest, Transport, TransportFail,
};

use crate::journal::{IntentState, Journal, Outcome};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::mint_origin::MintOrigin;
use super::steps;
use super::{Drive, Lane};

/// One directory answer. `Unknown` does not free the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Decision {
    /// The name is free, or the offline id was deleted.
    Free,
    /// Online or busy. Do not delete.
    Live,
    /// The directory did not answer. Do not fail the row.
    Unknown,
    /// This offline id was already removed for this subject. Do not delete it.
    Repeat,
}

/// Classify one JIT error. Do not acknowledge a name that was just removed.
///
/// A definite 409 with no directory access settles like any other JIT error.
/// Only a cleared name returns [`EnsureError::NameCleared`].
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
    origin: MintOrigin,
    error: SessionError,
) -> Result<Option<Started>, EnsureError>
where
    T: Transport + Lane,
{
    if !matches!(error, SessionError::Conflict) {
        return steps::fail_jit(journal, id, origin, error).await;
    }
    if !credentials(ctx) || !row_is_empty(journal, id).await? {
        return steps::fail_jit(journal, id, origin, error).await;
    }
    match decide(lane, ctx, journal, id, name).await? {
        Decision::Free | Decision::Repeat => {
            fail_cleared(journal, id).await?;
            Err(EnsureError::NameCleared)
        }
        Decision::Live => {
            fail_cleared(journal, id).await?;
            Err(EnsureError::Conflict)
        }
        Decision::Unknown => steps::hold(journal, id, EnsureError::Uncertain).await,
    }
}

/// Fail one empty row and prove its cleanup. Nothing was created to remove.
///
/// The proof releases the launch identity so a retry can bind the same name.
async fn fail_cleared(journal: &Journal, id: i64) -> Result<(), EnsureError> {
    journal
        .finish(id, Outcome::DefiniteFailure)
        .await
        .map_err(steps::map_journal)?;
    journal.record_cleanup(id).await.map_err(steps::map_journal)
}

/// True when a redelivered launch already deleted its runner id.
///
/// The failed row keeps the deleted id as a tombstone. The caller reports
/// [`EnsureError::NameCleared`] instead of holding the slot again.
pub(super) async fn cleared_repeat(journal: &Journal, id: i64) -> Result<bool, EnsureError> {
    let rows = journal.rows().await.map_err(steps::map_journal)?;
    Ok(rows.into_iter().any(|row| {
        row.id == id
            && row.state == IntentState::Failed
            && row
                .github_runner_id
                .as_deref()
                .is_some_and(|id| !id.is_empty())
    }))
}

pub(super) async fn release_empty<T>(
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
            fail_cleared(journal, id).await?;
            Ok(Decision::Free)
        }
        Decision::Repeat => {
            fail_cleared(journal, id).await?;
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
    // The journal subject is not the runner name on the job path.
    if recorded(journal, row, id).await? {
        return Ok(Decision::Repeat);
    }
    let decision = delete_offline(lane, &path, id, &ctx.pat)?;
    if decision == Decision::Free {
        remember(journal, row, id).await?;
    }
    Ok(decision)
}

async fn recorded(journal: &Journal, row_id: i64, runner_id: i64) -> Result<bool, EnsureError> {
    let needle = runner_id.to_string();
    let rows = journal.rows().await.map_err(steps::map_journal)?;
    let Some(subject) = rows
        .iter()
        .find(|row| row.id == row_id)
        .map(|row| row.subject.clone())
    else {
        return Ok(false);
    };
    Ok(rows.iter().any(|row| {
        row.kind == "launch"
            && row.subject == subject
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
