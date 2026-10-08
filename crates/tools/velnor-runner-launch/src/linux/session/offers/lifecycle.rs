//! Durable handling of actual Started/Completed identities in a queue batch.

use velnor_runner_github::InnerKind;
use velnor_runner_github::policy::ParsedTrustBatch;
use velnor_runner_journal::journal::Journal;

/// Persist every lifecycle identity and return exact Completed generations.
///
/// An event without a durable matching generation keeps the entire message
/// unacknowledged; request and expected runner names are never used as evidence.
pub(super) async fn persist_events(
    journal: &Journal,
    batch: &ParsedTrustBatch,
) -> Option<Vec<i64>> {
    let mut completed = Vec::new();
    for event in batch.events() {
        if !matches!(&event.job().kind, InnerKind::Started | InnerKind::Completed) {
            continue;
        }
        let job = event.job();
        let launch_id = match journal.observe_runner_event_with_id(job).await {
            Ok(Some(id)) => id,
            Ok(None) => exact_existing_generation(journal, job).await?,
            Err(_) => return None,
        };
        if matches!(&job.kind, InnerKind::Completed) && !completed.contains(&launch_id) {
            completed.push(launch_id);
        }
    }
    Some(completed)
}

async fn exact_existing_generation(
    journal: &Journal,
    event: &velnor_runner_github::InnerJob,
) -> Option<i64> {
    let (Some(name), Some(runner_id), Some(job_id), Some(run_id)) = (
        event.runner_name.as_deref(),
        event.runner_id,
        event.job_id.as_deref(),
        event.workflow_run_id,
    ) else {
        return None;
    };
    let rows = journal.rows().await.ok()?;
    let runner_id = runner_id.to_string();
    let mut matching = rows.iter().filter(|row| {
        row.kind == "launch"
            && row.runner_name.as_deref() == Some(name)
            && row.github_runner_id.as_deref() == Some(runner_id.as_str())
            && row.observed_job_id.as_deref() == Some(job_id)
            && row.observed_workflow_run_id == Some(run_id)
    });
    let row = matching.next()?;
    if matching.next().is_some() {
        return None;
    }
    Some(row.id)
}
