//! Durable journal adapter for host-owned worker cleanup.

use velnor_runner_journal::journal::{
    CleanupCheckpointIdentity, CleanupChildren, CleanupDiagnostics, CleanupStopPolicy, Journal,
    PostActionDisposition as JournalPostActions, RunnerStartObservation,
};

use crate::HostError;

use super::{
    ChildCleanupEvidence, ChildResourceInventory, ChildResourceKind, CleanupLedger, CleanupStep,
    DiagnosticsReceipt, PostActionDisposition, RunnerStopPolicy, WorkerGenerationIdentity,
    WorkerTerminationProof,
};

impl CleanupLedger for Journal {
    async fn begin(
        &self,
        identity: &WorkerGenerationIdentity,
        post_actions: &PostActionDisposition,
        stop_policy: &RunnerStopPolicy,
    ) -> Result<(), HostError> {
        let checkpoint = checkpoint_identity(identity)?;
        let post_actions = to_journal_post_actions(post_actions);
        let stop_policy = to_journal_stop_policy(stop_policy);
        self.begin_cleanup(&checkpoint, &post_actions, &stop_policy)
            .await
    }

    async fn runner_start_observation(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Option<RunnerStartObservation>, HostError> {
        self.cleanup_runner_start(identity.launch_id()).await
    }

    async fn record_runner_start_observation(
        &self,
        identity: &WorkerGenerationIdentity,
        observation: RunnerStartObservation,
    ) -> Result<(), HostError> {
        self.record_cleanup_runner_start(
            identity.launch_id(),
            identity.runner_container_id(),
            observation,
        )
        .await
    }

    async fn before(&self, launch_id: i64, step: &CleanupStep) -> Result<(), HostError> {
        self.cleanup_before(launch_id, &step_key(step)).await
    }

    async fn after(&self, launch_id: i64, step: &CleanupStep) -> Result<(), HostError> {
        self.cleanup_after(launch_id, &step_key(step)).await
    }

    async fn observe_children(
        &self,
        identity: &WorkerGenerationIdentity,
        inventory: &ChildResourceInventory,
    ) -> Result<(), HostError> {
        let mut containers = inventory.containers().to_vec();
        containers.sort_unstable();
        containers.dedup();
        let mut networks = inventory.networks().to_vec();
        networks.sort_unstable();
        networks.dedup();
        let children = CleanupChildren {
            containers,
            networks,
        };
        self.observe_cleanup_children(identity.launch_id(), &children)
            .await
    }

    async fn observed_children(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<ChildCleanupEvidence, HostError> {
        let children = self.cleanup_children(identity.launch_id()).await?;
        Ok(ChildCleanupEvidence {
            container_ids: children.containers,
            network_ids: children.networks,
        })
    }

    async fn prior_children_drained(
        &self,
        identity: &WorkerGenerationIdentity,
    ) -> Result<Option<ChildCleanupEvidence>, HostError> {
        self.prior_cleanup_children_drained(identity.launch_id())
            .await?
            .map(|children| {
                Ok(ChildCleanupEvidence {
                    container_ids: children.containers,
                    network_ids: children.networks,
                })
            })
            .transpose()
    }

    async fn children_drained(
        &self,
        identity: &WorkerGenerationIdentity,
        evidence: &ChildCleanupEvidence,
    ) -> Result<(), HostError> {
        let children = CleanupChildren {
            containers: evidence.container_ids().to_vec(),
            networks: evidence.network_ids().to_vec(),
        };
        self.record_cleanup_children_drained(identity.launch_id(), &children)
            .await
    }

    async fn diagnostics(
        &self,
        launch_id: i64,
        receipt: &DiagnosticsReceipt,
    ) -> Result<(), HostError> {
        self.record_cleanup_diagnostics(
            launch_id,
            &CleanupDiagnostics {
                relative_path: receipt.relative_path().to_owned(),
                sha256: receipt.sha256().to_owned(),
                bytes: receipt.bytes(),
                redacted: receipt.redacted(),
                retained: receipt.retained(),
                source_absent: receipt.source_absent(),
            },
        )
        .await
    }

    async fn complete(&self, proof: &WorkerTerminationProof) -> Result<(), HostError> {
        self.record_physical_cleanup(proof).await
    }
}

fn checkpoint_identity(
    identity: &WorkerGenerationIdentity,
) -> Result<CleanupCheckpointIdentity, HostError> {
    let observed = identity.observed_job();
    Ok(CleanupCheckpointIdentity {
        launch_id: identity.launch_id(),
        expected_runner_name: identity.expected_runner_name().to_owned(),
        worker_volume: identity.worker_volume().to_owned(),
        runner_container_id: identity.runner_container_id().to_owned(),
        dind_container_id: identity.dind_container_id().to_owned(),
        outer_network_name: identity.outer_network_name().map(str::to_owned),
        outer_network_id: identity.outer_network_id().map(str::to_owned),
        observed_workflow_run_id: observed
            .map(|job| checked_journal_id(job.workflow_run_id()))
            .transpose()?,
        observed_attempt: observed
            .and_then(super::ObservedJobIdentity::attempt)
            .map(i64::from),
        observed_job_id: observed.map(|job| job.scale_set_job_id().to_owned()),
        observed_actions_job_id: observed
            .and_then(super::ObservedJobIdentity::actions_job_id)
            .map(checked_journal_id)
            .transpose()?,
        observed_runner_id: observed
            .map(|job| checked_journal_id(job.runner_id()))
            .transpose()?,
        observed_runner_name: observed.map(|job| job.runner_name().to_owned()),
    })
}

fn checked_journal_id(value: u64) -> Result<i64, HostError> {
    i64::try_from(value).map_err(|_| HostError::Identity)
}

fn to_journal_post_actions(value: &PostActionDisposition) -> JournalPostActions {
    match value {
        PostActionDisposition::Completed => JournalPostActions::Completed,
        PostActionDisposition::NotRun => JournalPostActions::NotRun,
        PostActionDisposition::Interrupted { reason_class } => JournalPostActions::Interrupted {
            reason_class: reason_class.clone(),
        },
        PostActionDisposition::Unknown => JournalPostActions::Unknown,
    }
}

fn to_journal_stop_policy(value: &RunnerStopPolicy) -> CleanupStopPolicy {
    match value {
        RunnerStopPolicy::RequireStopped => CleanupStopPolicy::RequireStopped,
        RunnerStopPolicy::StopAtDeadline {
            grace_seconds,
            reason_class,
        } => CleanupStopPolicy::StopAtDeadline {
            grace_seconds: *grace_seconds,
            reason_class: reason_class.clone(),
        },
    }
}

fn step_key(step: &CleanupStep) -> String {
    match step {
        CleanupStep::RunnerTermination => "runner-termination".to_owned(),
        CleanupStep::DiagnosticsRetention => "diagnostics-retention".to_owned(),
        CleanupStep::ChildEnumeration => "child-enumeration".to_owned(),
        CleanupStep::ChildrenDrained => "children-drained".to_owned(),
        CleanupStep::DindTermination => "dind-termination".to_owned(),
        CleanupStep::ChildResourceRemoval {
            id,
            kind: ChildResourceKind::Container,
        } => format!("child-container:{id}"),
        CleanupStep::ChildResourceRemoval {
            id,
            kind: ChildResourceKind::Network,
        } => format!("child-network:{id}"),
        CleanupStep::RunnerRemoval => "runner-removal".to_owned(),
        CleanupStep::DindRemoval => "dind-removal".to_owned(),
        CleanupStep::OuterNetworkRemoval => "outer-network-removal".to_owned(),
        CleanupStep::VolumeRemoval => "volume-removal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{checkpoint_identity, step_key};
    use crate::worker::cleanup::{
        ChildResourceKind, CleanupStep, ObservedJobIdentity, WorkerGenerationIdentity,
    };

    #[test]
    fn cleanup_step_keys_are_stable_and_do_not_use_debug_formatting() {
        let cases = [
            (CleanupStep::RunnerTermination, "runner-termination"),
            (CleanupStep::DiagnosticsRetention, "diagnostics-retention"),
            (CleanupStep::ChildEnumeration, "child-enumeration"),
            (CleanupStep::ChildrenDrained, "children-drained"),
            (CleanupStep::DindTermination, "dind-termination"),
            (
                CleanupStep::ChildResourceRemoval {
                    id: "a123456789abcdef".to_owned(),
                    kind: ChildResourceKind::Container,
                },
                "child-container:a123456789abcdef",
            ),
            (
                CleanupStep::ChildResourceRemoval {
                    id: "b123456789abcdef".to_owned(),
                    kind: ChildResourceKind::Network,
                },
                "child-network:b123456789abcdef",
            ),
            (CleanupStep::RunnerRemoval, "runner-removal"),
            (CleanupStep::DindRemoval, "dind-removal"),
            (CleanupStep::OuterNetworkRemoval, "outer-network-removal"),
            (CleanupStep::VolumeRemoval, "volume-removal"),
        ];
        for (step, expected) in cases {
            assert_eq!(step_key(&step), expected);
        }
    }

    #[test]
    fn checkpoint_identity_keeps_event_and_rest_fields_distinct() -> Result<(), crate::HostError> {
        let observed = ObservedJobIdentity::new(
            73,
            Some(4),
            "opaque-scale-set-job".to_owned(),
            Some(8_765),
            29,
            "velnor-17".to_owned(),
        )?;
        let identity = WorkerGenerationIdentity::new(
            17,
            "velnor-17".to_owned(),
            "w0123456789abcdef0123456789abcdef".to_owned(),
            "a123456789abcdef".to_owned(),
            "b123456789abcdef".to_owned(),
            Some(observed),
        )?;
        let checkpoint = checkpoint_identity(&identity)?;

        assert_eq!(checkpoint.observed_workflow_run_id, Some(73));
        assert_eq!(checkpoint.observed_attempt, Some(4));
        assert_eq!(
            checkpoint.observed_job_id.as_deref(),
            Some("opaque-scale-set-job")
        );
        assert_eq!(checkpoint.observed_actions_job_id, Some(8_765));
        assert_eq!(checkpoint.observed_runner_id, Some(29));
        assert_eq!(
            checkpoint.observed_runner_name.as_deref(),
            Some("velnor-17")
        );
        assert_eq!(checkpoint.outer_network_name, None);
        assert_eq!(checkpoint.outer_network_id, None);
        Ok(())
    }
}
