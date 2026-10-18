//! The open scale-set session id. Restart deletes that id and nothing else.

use velnor_runner_github::{QueueSession, delete_session, reopen_session};

use crate::error::HostError;
use crate::journal::{ConnectionJournal, Outcome};
use crate::listen::{Link, OWNER_NAME, annotate, map_listen};
use crate::scale_set::EnsureError;

const KIND: &str = "session";

/// Create one scoped session after storing its create intent.
pub(super) async fn open_connection_session(
    link: &mut Link,
    set_id: i64,
    admin_token: &str,
    journal: &ConnectionJournal,
) -> Result<(QueueSession, i64), EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    if rows
        .iter()
        .any(|row| row.kind == "session-create" && !row.cleanup_proven)
    {
        return Err(EnsureError::Uncertain);
    }
    let leaked = connection_sessions(&rows);
    let refs: Vec<&str> = leaked.iter().map(|(_, id)| id.as_str()).collect();
    let create = journal
        .begin("session-create", &uuid::Uuid::new_v4().simple().to_string())
        .await
        .map_err(map_journal)?;
    let session = match reopen_session(link.transport(), set_id, OWNER_NAME, admin_token, &refs) {
        Ok(session) => session,
        Err(error) => {
            journal
                .finish(create, Outcome::Uncertain)
                .await
                .map_err(map_journal)?;
            return Err(annotate(error, "create-session"));
        }
    };
    let row = match journal.begin(KIND, &session.session_id).await {
        Ok(row) => row,
        Err(error) => {
            let deleted =
                delete_session(link.transport(), set_id, &session.session_id, admin_token);
            if deleted.is_ok() {
                journal
                    .finish(create, Outcome::Done)
                    .await
                    .map_err(map_journal)?;
            } else {
                journal
                    .finish(create, Outcome::Uncertain)
                    .await
                    .map_err(map_journal)?;
            }
            return Err(map_journal(error));
        }
    };
    journal
        .finish(create, Outcome::Done)
        .await
        .map_err(map_journal)?;
    close_connection_rows(journal, &leaked).await?;
    Ok((session, row))
}

/// Delete one scoped session and record its exact cleanup.
pub(super) async fn close_connection_session(
    link: &mut Link,
    set_id: i64,
    session_id: &str,
    row: i64,
    admin_token: &str,
    journal: &ConnectionJournal,
) -> Result<(), EnsureError> {
    delete_session(link.transport(), set_id, session_id, admin_token).map_err(map_listen)?;
    journal.record_cleanup(row).await.map_err(map_journal)
}

fn connection_sessions(rows: &[crate::reconcile::IntentRow]) -> Vec<(i64, String)> {
    rows.iter()
        .filter(|row| row.kind == KIND && !row.cleanup_proven && !row.subject.is_empty())
        .map(|row| (row.id, row.subject.clone()))
        .collect()
}

async fn close_connection_rows(
    journal: &ConnectionJournal,
    rows: &[(i64, String)],
) -> Result<(), EnsureError> {
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
