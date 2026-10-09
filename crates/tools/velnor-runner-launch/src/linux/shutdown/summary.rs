use std::time::Instant;

use tokio::time::{Instant as TokioInstant, timeout_at};

use velnor_runner_host::{DockerDaemonBinding, worker::OwnedDockerResource};
use velnor_runner_journal::journal::{IntentState, Journal, LaunchEffectState};
use velnor_runner_launch_slot::holds;

use super::super::{LinuxAdmissionState, LinuxDaemonOutcome, LinuxShutdownGap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RowCounts {
    pub occupied_launches: usize,
    pub unresolved_intents: usize,
}

pub(super) async fn shutdown_summary(
    journal: &Journal,
    admission: LinuxAdmissionState,
    docker_binding: &DockerDaemonBinding,
    deadline: Instant,
    cleaned: usize,
    previous_resources: usize,
) -> Result<LinuxDaemonOutcome, super::super::LinuxDaemonError> {
    if Instant::now() >= deadline {
        return Ok(unresolved_outcome(
            journal,
            admission,
            LinuxShutdownGap::QuiescenceSnapshotPastDeadline,
            cleaned,
            Some(previous_resources),
            deadline,
        )
        .await);
    }
    let tokio_deadline = TokioInstant::from_std(deadline);
    let Ok(Ok(rows)) = timeout_at(tokio_deadline, journal.rows()).await else {
        return Ok(outcome_with_counts(
            admission,
            LinuxShutdownGap::JournalUnavailable,
            None,
            Some(previous_resources),
            cleaned,
            Some(deadline),
        ));
    };
    let Ok(Ok(resources)) = timeout_at(
        tokio_deadline,
        velnor_runner_host::worker::list_owned_docker_resources_bound_until(
            docker_binding,
            tokio_deadline,
        ),
    )
    .await
    else {
        return Ok(outcome_with_counts(
            admission,
            LinuxShutdownGap::DockerInventoryUnavailable,
            Some(row_counts(&rows)),
            Some(previous_resources),
            cleaned,
            Some(deadline),
        ));
    };
    Ok(summarize_snapshot_before_deadline(
        admission, &rows, &resources, cleaned, deadline,
    ))
}

pub(crate) fn summarize_snapshot_before_deadline(
    admission: LinuxAdmissionState,
    rows: &[velnor_runner_host::IntentRow],
    resources: &[OwnedDockerResource],
    cleaned: usize,
    deadline: Instant,
) -> LinuxDaemonOutcome {
    if Instant::now() >= deadline {
        return outcome_with_counts(
            admission,
            LinuxShutdownGap::QuiescenceSnapshotPastDeadline,
            Some(row_counts(rows)),
            Some(resources.len()),
            cleaned,
            Some(deadline),
        );
    }
    let outcome = summarize_complete_snapshot(admission, rows, resources, cleaned);
    if matches!(outcome, LinuxDaemonOutcome::Quiescent { .. }) && Instant::now() >= deadline {
        return outcome_with_counts(
            admission,
            LinuxShutdownGap::QuiescenceSnapshotPastDeadline,
            Some(row_counts(rows)),
            Some(resources.len()),
            cleaned,
            Some(deadline),
        );
    }
    outcome
}

pub(crate) fn summarize_complete_snapshot(
    admission: LinuxAdmissionState,
    rows: &[velnor_runner_host::IntentRow],
    resources: &[OwnedDockerResource],
    cleaned: usize,
) -> LinuxDaemonOutcome {
    let counts = row_counts(rows);
    if counts.occupied_launches == 0 && counts.unresolved_intents == 0 && resources.is_empty() {
        return LinuxDaemonOutcome::Quiescent {
            admission,
            cleaned_generations: cleaned,
        };
    }
    LinuxDaemonOutcome::Unresolved {
        admission,
        gap: if resources.is_empty() {
            LinuxShutdownGap::UnresolvedIntent
        } else {
            LinuxShutdownGap::OwnedResourcesRemain
        },
        occupied_launches: Some(counts.occupied_launches),
        unresolved_intents: Some(counts.unresolved_intents),
        owned_resources: Some(resources.len()),
        cleaned_generations: cleaned,
    }
}

pub(crate) async fn unresolved_outcome(
    journal: &Journal,
    admission: LinuxAdmissionState,
    gap: LinuxShutdownGap,
    cleaned: usize,
    resources: Option<usize>,
    deadline: Instant,
) -> LinuxDaemonOutcome {
    let counts = timeout_at(TokioInstant::from_std(deadline), journal.rows())
        .await
        .ok()
        .and_then(Result::ok)
        .map(|rows| row_counts(&rows));
    outcome_with_counts(admission, gap, counts, resources, cleaned, Some(deadline))
}

pub(crate) fn outcome_with_counts(
    admission: LinuxAdmissionState,
    gap: LinuxShutdownGap,
    counts: Option<RowCounts>,
    resources: Option<usize>,
    cleaned: usize,
    deadline: Option<Instant>,
) -> LinuxDaemonOutcome {
    let occupied_launches = counts.map(|counts| counts.occupied_launches);
    let unresolved_intents = counts.map(|counts| counts.unresolved_intents);
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return LinuxDaemonOutcome::Deadline {
            admission,
            occupied_launches,
            unresolved_intents,
            gap,
            owned_resources: resources,
        };
    }
    LinuxDaemonOutcome::Unresolved {
        admission,
        gap,
        occupied_launches,
        unresolved_intents,
        owned_resources: resources,
        cleaned_generations: cleaned,
    }
}

pub(crate) fn row_counts(rows: &[velnor_runner_host::IntentRow]) -> RowCounts {
    RowCounts {
        occupied_launches: rows
            .iter()
            .filter(|row| row.kind == "launch" && holds(row))
            .count(),
        unresolved_intents: rows
            .iter()
            .filter(|row| row.kind != "launch" && intent_unresolved(row))
            .count(),
    }
}

fn intent_unresolved(row: &velnor_runner_host::IntentRow) -> bool {
    if row.cleanup_proven {
        return false;
    }
    if row.kind == "discovery-credential" {
        return matches!(row.state, IntentState::Pending | IntentState::Uncertain);
    }
    if row.state == IntentState::Failed && row.launch_effect == LaunchEffectState::DefiniteNoEffect
    {
        return false;
    }
    true
}
