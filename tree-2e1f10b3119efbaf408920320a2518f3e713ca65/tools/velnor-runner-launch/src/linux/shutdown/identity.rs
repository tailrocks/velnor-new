use velnor_runner_host::worker::{ObservedJobIdentity, WorkerGenerationIdentity};
use velnor_runner_journal::journal::{IntentState, LaunchEffectState, RunnerStartIntent};

pub(crate) fn cleanup_identity(
    row: &velnor_runner_host::IntentRow,
) -> Option<WorkerGenerationIdentity> {
    if row.kind != "launch"
        || row.state != IntentState::Done
        || row.launch_effect != LaunchEffectState::MayHaveEffect
        || !row.remote_terminal
        || row.cleanup_proven
        || row.runner_start_intent != RunnerStartIntent::MayHaveStarted
    {
        return None;
    }
    let launch_id = row.id;
    let runner_name = row.runner_name.as_ref()?.clone();
    let worker_volume = row.worker_volume.as_ref()?.clone();
    let runner_id = row.docker_id.as_ref()?.clone();
    let dind_id = row.dind_id.as_ref()?.clone();
    let observed_job = ObservedJobIdentity::new(
        u64::try_from(row.observed_workflow_run_id?).ok()?,
        None,
        row.observed_job_id.as_ref()?.clone(),
        None,
        row.github_runner_id.as_ref()?.parse().ok()?,
        runner_name.clone(),
    )
    .ok()?;
    WorkerGenerationIdentity::new_with_network(
        launch_id,
        runner_name,
        worker_volume,
        runner_id,
        dind_id,
        row.outer_network_name.clone(),
        row.outer_network_id.clone(),
        Some(observed_job),
    )
    .ok()
}
