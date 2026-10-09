use crate::journal::{
    CleanupCheckpointIdentity, CleanupDiagnostics, CleanupDisposition, PhysicalCleanupProof,
    PostActionDisposition, RunnerStartObservation,
};

pub(super) const RUNNER_ID: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const DIND_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
pub(super) const OUTER_NETWORK_ID: &str =
    "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
pub(super) const CHILD_CONTAINER_ID: &str =
    "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
pub(super) const CHILD_NETWORK_ID: &str =
    "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub(super) struct TestProof {
    launch_id: i64,
    runner_name: String,
    volume: String,
    network_name: String,
    diagnostics_path: String,
    child_containers: Vec<String>,
    child_networks: Vec<String>,
    absent_volumes: Vec<String>,
    post_actions: PostActionDisposition,
    observed_runner_id: i64,
    observed_job_id: String,
    observed_attempt: Option<i64>,
    observed_actions_job_id: Option<i64>,
}

impl TestProof {
    pub(super) fn new(
        launch_id: i64,
        runner_name: String,
        volume: String,
        network_name: String,
        post_actions: PostActionDisposition,
    ) -> Self {
        let absent_volumes = volume_names(&volume);
        Self {
            launch_id,
            runner_name,
            volume,
            network_name,
            diagnostics_path: format!("launch-{launch_id}/runner-diagnostics.tar"),
            child_containers: vec![CHILD_CONTAINER_ID.to_owned()],
            child_networks: vec![CHILD_NETWORK_ID.to_owned()],
            absent_volumes,
            post_actions,
            observed_runner_id: 99,
            observed_job_id: "observed-job".to_owned(),
            observed_attempt: None,
            observed_actions_job_id: None,
        }
    }

    pub(super) fn with_observed_actions(
        mut self,
        runner_id: i64,
        scale_set_job_id: &str,
        attempt: i64,
        actions_job_id: i64,
    ) -> Self {
        self.observed_runner_id = runner_id;
        self.observed_job_id = scale_set_job_id.to_owned();
        self.observed_attempt = Some(attempt);
        self.observed_actions_job_id = Some(actions_job_id);
        self
    }

    pub(super) fn identity(&self) -> CleanupCheckpointIdentity {
        CleanupCheckpointIdentity {
            launch_id: self.launch_id,
            expected_runner_name: self.runner_name.clone(),
            worker_volume: self.volume.clone(),
            runner_container_id: RUNNER_ID.to_owned(),
            dind_container_id: DIND_ID.to_owned(),
            outer_network_name: Some(self.network_name.clone()),
            outer_network_id: Some(OUTER_NETWORK_ID.to_owned()),
            observed_workflow_run_id: Some(45),
            observed_attempt: self.observed_attempt,
            observed_job_id: Some(self.observed_job_id.clone()),
            observed_actions_job_id: self.observed_actions_job_id,
            observed_runner_id: Some(self.observed_runner_id),
            observed_runner_name: Some(self.runner_name.clone()),
        }
    }

    pub(super) fn diagnostics(&self) -> CleanupDiagnostics {
        CleanupDiagnostics {
            relative_path: self.diagnostics_path.clone(),
            sha256: DIGEST.to_owned(),
            bytes: 17,
            redacted: true,
            retained: true,
            source_absent: false,
        }
    }
}

impl PhysicalCleanupProof for TestProof {
    fn launch_id(&self) -> i64 {
        self.launch_id
    }
    fn expected_runner_name(&self) -> &str {
        &self.runner_name
    }
    fn worker_volume(&self) -> &str {
        &self.volume
    }
    fn runner_container_id(&self) -> &str {
        RUNNER_ID
    }
    fn dind_container_id(&self) -> &str {
        DIND_ID
    }
    fn outer_network_name(&self) -> Option<&str> {
        Some(&self.network_name)
    }
    fn outer_network_id(&self) -> Option<&str> {
        Some(OUTER_NETWORK_ID)
    }
    fn outer_network_absent(&self) -> bool {
        true
    }
    fn observed_workflow_run_id(&self) -> Option<i64> {
        Some(45)
    }
    fn observed_attempt(&self) -> Option<i64> {
        self.observed_attempt
    }
    fn observed_job_id(&self) -> Option<&str> {
        Some(&self.observed_job_id)
    }
    fn observed_actions_job_id(&self) -> Option<i64> {
        self.observed_actions_job_id
    }
    fn observed_runner_id(&self) -> Option<i64> {
        Some(self.observed_runner_id)
    }
    fn observed_runner_name(&self) -> Option<&str> {
        Some(&self.runner_name)
    }
    fn launch_fenced(&self) -> bool {
        true
    }
    fn runner_start_observation(&self) -> RunnerStartObservation {
        RunnerStartObservation::MayHaveStarted
    }
    fn all_owned_children_networks_and_volumes_absent(&self) -> bool {
        true
    }
    fn diagnostics_relative_path(&self) -> &str {
        &self.diagnostics_path
    }
    fn diagnostics_sha256(&self) -> &str {
        DIGEST
    }
    fn diagnostics_bytes(&self) -> u64 {
        17
    }
    fn diagnostics_redacted(&self) -> bool {
        true
    }
    fn diagnostics_retained(&self) -> bool {
        true
    }
    fn diagnostics_source_absent(&self) -> bool {
        false
    }
    fn removed_child_container_ids(&self) -> &[String] {
        &self.child_containers
    }
    fn removed_child_network_ids(&self) -> &[String] {
        &self.child_networks
    }
    fn absent_volume_names(&self) -> &[String] {
        &self.absent_volumes
    }
    fn post_actions(&self) -> PostActionDisposition {
        self.post_actions.clone()
    }
    fn cleanup_disposition(&self) -> CleanupDisposition {
        CleanupDisposition::Interrupted {
            reason_class: "cleanup_complete".to_owned(),
        }
    }
}

fn volume_names(base: &str) -> Vec<String> {
    let mut names = vec![
        base.to_owned(),
        format!("{base}-work"),
        format!("{base}-externals"),
        format!("{base}-docker"),
        format!("{base}-home"),
        format!("{base}-tmp"),
    ];
    names.sort_unstable();
    names
}
