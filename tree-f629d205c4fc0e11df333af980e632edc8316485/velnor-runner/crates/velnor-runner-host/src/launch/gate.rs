//! Opt-in restart gate. Counts only. No container deletes and no tokens.

use bollard::Docker;

use crate::error::HostError;
use crate::journal::Journal;
use crate::reconcile::{IntentRow, Reconcile, before_advertise};
use crate::scale_set::EnsureError;

use super::inspect::container_running;

/// `hold occupied=N adopt=M` or `advertise occupied=N`.
///
/// Adopt is a count. Full docker ids are not printed.
#[must_use]
pub(crate) fn gate_line(decision: &Reconcile) -> String {
    match decision {
        Reconcile::Hold { adopt, occupied } => {
            let adopt_count = adopt.len();
            format!("hold occupied={occupied} adopt={adopt_count}")
        }
        Reconcile::Advertise { occupied } => format!("advertise occupied={occupied}"),
    }
}

/// Compare the journal with running containers, then advertise or hold.
///
/// Rows are loaded before any inspect, so the database connection is not held
/// across Docker. Only a Docker 404 means not running; incomplete observations
/// and other inspect failures return an error. Nothing is deleted.
///
/// # Errors
///
/// Returns [`EnsureError`] when the journal cannot be read or an inspect result
/// cannot establish the container's running state.
pub(crate) async fn reconcile_gate(
    journal: &Journal,
    docker: &Docker,
) -> Result<Reconcile, EnsureError> {
    let rows = journal.rows().await.map_err(map_journal)?;
    let observed_docker = running_ids(&rows, docker).await?;
    Ok(before_advertise(
        &rows,
        &str_refs(&observed_docker),
        &github_ids(&rows),
        &owned_ids(&rows),
    ))
}

fn str_refs(ids: &[String]) -> Vec<&str> {
    ids.iter().map(String::as_str).collect()
}

fn github_ids(rows: &[IntentRow]) -> Vec<&str> {
    rows.iter()
        .filter_map(|row| row.github_runner_id.as_deref())
        .filter(|id| !id.is_empty())
        .collect()
}

fn owned_ids(rows: &[IntentRow]) -> Vec<&str> {
    rows.iter()
        .filter_map(|row| row.docker_id.as_deref())
        .collect()
}

async fn running_ids(rows: &[IntentRow], docker: &Docker) -> Result<Vec<String>, EnsureError> {
    let mut running = Vec::new();
    for row in rows {
        let Some(id) = row.docker_id.as_deref().filter(|id| !id.is_empty()) else {
            continue;
        };
        if container_running(docker, id).await? {
            running.push(id.to_owned());
        }
    }
    Ok(running)
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

#[cfg(test)]
mod tests {
    use super::gate_line;
    use crate::Reconcile;

    #[test]
    fn line_counts_and_hides_docker_ids() {
        let hold = Reconcile::Hold {
            adopt: vec!["0123456789abcdef".to_owned()],
            occupied: 2,
        };
        let line = gate_line(&hold);
        assert_eq!(line, "hold occupied=2 adopt=1");
        assert!(!line.contains("0123456789abcdef"));
        let open = Reconcile::Advertise { occupied: 0 };
        assert_eq!(gate_line(&open), "advertise occupied=0");
    }
}
