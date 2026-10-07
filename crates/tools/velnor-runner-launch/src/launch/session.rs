//! Scale-set session lifetime and quarantine of unresolved delivery context.

use std::future::Future;

use velnor_runner_github::{QueueSession, delete_session, reopen_session};

use velnor_runner_host::HostError;
use velnor_runner_host::listen::{Link, OWNER_NAME, annotate, map_listen};
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_journal::journal::Journal;

const KIND: &str = "session";

mod tests;

/// Create a session only when the journal has no unresolved session rows.
///
/// # Errors
///
/// Returns [`EnsureError`] when the journal or the session call fails.
/// An unresolved session returns [`EnsureError::Uncertain`] without creating a
/// replacement. The queue API does not provide an authoritative reattach or
/// expiry operation, so the old delivery context is retained.
///
/// A failed remember deletes the session this call just created.
pub(super) async fn open_session(
    link: &mut Link,
    set_id: i64,
    admin_token: &str,
    journal: &Journal,
) -> Result<(QueueSession, i64), EnsureError> {
    let leaked = leaked(journal).await?;
    create_only_if_resolved(&leaked, || async {
        let session = reopen_session(link.transport(), set_id, OWNER_NAME, admin_token, &[])
            .map_err(|err| annotate(err, "create-session"))?;
        let row = remember(link, set_id, admin_token, journal, &session).await?;
        Ok((session, row))
    })
    .await
}

/// Fence launch preflight while any prior session delivery context is unresolved.
pub(super) async fn require_no_unresolved(journal: &Journal) -> Result<(), EnsureError> {
    if leaked(journal).await?.is_empty() {
        Ok(())
    } else {
        Err(EnsureError::Uncertain)
    }
}

async fn create_only_if_resolved<T, Create, Created>(
    unresolved: &[(i64, String)],
    create: Create,
) -> Result<T, EnsureError>
where
    Create: FnOnce() -> Created,
    Created: Future<Output = Result<T, EnsureError>>,
{
    if !unresolved.is_empty() {
        return Err(EnsureError::Uncertain);
    }
    create().await
}

/// Close a session only after the poll and all message work completed safely.
pub(super) async fn close_after_poll<T, Close, Closed>(
    poll: &Result<T, EnsureError>,
    retain_session: bool,
    close: Close,
) -> Result<(), EnsureError>
where
    Close: FnOnce() -> Closed,
    Closed: Future<Output = Result<(), EnsureError>>,
{
    if poll.is_err() || retain_session {
        return Ok(());
    }
    close().await
}

/// Delete the session this process opened, then mark that row cleaned.
///
/// # Errors
///
/// Returns [`EnsureError`] when delete or the journal write fails. The row
/// stays unresolved when deletion fails, and future session creation remains
/// fenced because the queue API exposes no authoritative reattach operation.
pub(super) async fn close_session(
    link: &mut Link,
    set_id: i64,
    session_id: &str,
    row: i64,
    admin_token: &str,
    journal: &Journal,
) -> Result<(), EnsureError> {
    delete_session(link.transport(), set_id, session_id, admin_token).map_err(map_listen)?;
    journal.record_cleanup(row).await.map_err(map_journal)
}

async fn remember(
    link: &mut Link,
    set_id: i64,
    admin_token: &str,
    journal: &Journal,
    session: &QueueSession,
) -> Result<i64, EnsureError> {
    match journal.begin(KIND, &session.session_id).await {
        Ok(row) => Ok(row),
        Err(error) => {
            delete_session(link.transport(), set_id, &session.session_id, admin_token)
                .map_err(map_listen)?;
            Err(map_journal(error))
        }
    }
}

async fn leaked(journal: &Journal) -> Result<Vec<(i64, String)>, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    Ok(rows
        .into_iter()
        .filter(|row| row.kind == KIND && !row.cleanup_proven)
        .map(|row| (row.id, row.subject))
        .collect())
}

fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
