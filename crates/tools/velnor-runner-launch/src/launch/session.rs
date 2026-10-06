//! The open scale-set session id. Restart deletes that id and nothing else.

use velnor_runner_github::{QueueSession, delete_session, reopen_session};

use velnor_runner_host::HostError;
use velnor_runner_host::journal::Journal;
use velnor_runner_host::listen::{Link, OWNER_NAME, annotate, map_listen};
use velnor_runner_host::scale_set::EnsureError;

const KIND: &str = "session";

/// Drop recorded session ids, create one session, and store its id.
///
/// # Errors
///
/// Returns [`EnsureError`] when the journal or the session call fails.
/// A failed remember deletes the session this call just created.
pub(super) async fn open_session(
    link: &mut Link,
    set_id: i64,
    admin_token: &str,
    journal: &Journal,
) -> Result<(QueueSession, i64), EnsureError> {
    let leaked = leaked(journal).await?;
    let refs: Vec<&str> = leaked.iter().map(|(_, id)| id.as_str()).collect();
    let session = reopen_session(link.transport(), set_id, OWNER_NAME, admin_token, &refs)
        .map_err(|err| annotate(err, "create-session"))?;
    let row = remember(link, set_id, admin_token, journal, &session).await?;
    close_rows(journal, &leaked).await?;
    Ok((session, row))
}

/// Delete the session this process opened, then mark that row cleaned.
///
/// # Errors
///
/// Returns [`EnsureError`] when delete or the journal write fails.
/// The row stays open when delete fails, so the next open can retry it.
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
        .filter(|row| row.kind == KIND && !row.cleanup_proven && !row.subject.is_empty())
        .map(|row| (row.id, row.subject))
        .collect())
}

async fn close_rows(journal: &Journal, rows: &[(i64, String)]) -> Result<(), EnsureError> {
    for (id, _) in rows {
        journal.record_cleanup(*id).await.map_err(map_journal)?;
    }
    Ok(())
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
