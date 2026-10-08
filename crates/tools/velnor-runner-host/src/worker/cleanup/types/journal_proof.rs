// Journal adapter for locally sealed worker cleanup proof.
impl velnor_runner_journal::journal::PhysicalCleanupProof for WorkerTerminationProof {
    fn launch_id(&self) -> i64 {
        WorkerTerminationProof::launch_id(self)
    }

    fn expected_runner_name(&self) -> &str {
        WorkerTerminationProof::expected_runner_name(self)
    }

    fn worker_volume(&self) -> &str {
        WorkerTerminationProof::worker_volume(self)
    }

    fn runner_container_id(&self) -> &str {
        WorkerTerminationProof::runner_container_id(self)
    }

    fn dind_container_id(&self) -> &str {
        WorkerTerminationProof::dind_container_id(self)
    }

    fn outer_network_name(&self) -> Option<&str> {
        WorkerTerminationProof::outer_network(self).map(|(name, _id)| name)
    }

    fn outer_network_id(&self) -> Option<&str> {
        WorkerTerminationProof::outer_network(self).map(|(_name, id)| id)
    }

    fn outer_network_absent(&self) -> bool {
        WorkerTerminationProof::outer_network_absent(self)
    }

    fn observed_workflow_run_id(&self) -> Option<i64> {
        WorkerTerminationProof::workflow_run_id(self).and_then(|run_id| i64::try_from(run_id).ok())
    }

    fn observed_attempt(&self) -> Option<i64> {
        WorkerTerminationProof::attempt(self).map(i64::from)
    }

    fn observed_job_id(&self) -> Option<&str> {
        WorkerTerminationProof::scale_set_job_id(self)
    }

    fn observed_actions_job_id(&self) -> Option<i64> {
        WorkerTerminationProof::actions_job_id(self).and_then(|job_id| i64::try_from(job_id).ok())
    }

    fn observed_runner_id(&self) -> Option<i64> {
        WorkerTerminationProof::observed_runner_id(self)
            .and_then(|runner_id| i64::try_from(runner_id).ok())
    }

    fn observed_runner_name(&self) -> Option<&str> {
        WorkerTerminationProof::observed_runner_name(self)
    }

    fn launch_fenced(&self) -> bool {
        WorkerTerminationProof::launch_fenced(self)
    }

    fn runner_start_observation(&self) -> velnor_runner_journal::journal::RunnerStartObservation {
        WorkerTerminationProof::runner_start_observation(self)
    }

    fn all_owned_children_networks_and_volumes_absent(&self) -> bool {
        WorkerTerminationProof::all_owned_children_networks_and_volumes_absent(self)
    }

    fn diagnostics_relative_path(&self) -> &str {
        WorkerTerminationProof::diagnostics_relative_path(self)
    }

    fn diagnostics_sha256(&self) -> &str {
        WorkerTerminationProof::diagnostics_sha256(self)
    }

    fn diagnostics_bytes(&self) -> u64 {
        WorkerTerminationProof::diagnostics_bytes(self)
    }

    fn diagnostics_redacted(&self) -> bool {
        WorkerTerminationProof::diagnostics_redacted(self)
    }

    fn diagnostics_retained(&self) -> bool {
        WorkerTerminationProof::diagnostics_retained(self)
    }

    fn diagnostics_source_absent(&self) -> bool {
        WorkerTerminationProof::diagnostics_source_absent(self)
    }

    fn removed_child_container_ids(&self) -> &[String] {
        WorkerTerminationProof::children(self).container_ids()
    }

    fn removed_child_network_ids(&self) -> &[String] {
        WorkerTerminationProof::children(self).network_ids()
    }

    fn absent_volume_names(&self) -> &[String] {
        WorkerTerminationProof::absent_volumes(self)
    }

    fn post_actions(&self) -> velnor_runner_journal::journal::PostActionDisposition {
        use velnor_runner_journal::journal::PostActionDisposition as JournalPostActions;

        match WorkerTerminationProof::post_actions(self) {
            PostActionDisposition::Completed => JournalPostActions::Completed,
            PostActionDisposition::NotRun => JournalPostActions::NotRun,
            PostActionDisposition::Interrupted { reason_class } => {
                JournalPostActions::Interrupted {
                    reason_class: reason_class.clone(),
                }
            }
            PostActionDisposition::Unknown => JournalPostActions::Unknown,
        }
    }

    fn cleanup_disposition(&self) -> velnor_runner_journal::journal::CleanupDisposition {
        use velnor_runner_journal::journal::CleanupDisposition as JournalCleanup;

        match WorkerTerminationProof::cleanup_disposition(self) {
            CleanupDisposition::Completed => JournalCleanup::Completed,
            CleanupDisposition::Interrupted { reason_class } => JournalCleanup::Interrupted {
                reason_class: reason_class.to_owned(),
            },
        }
    }
}
