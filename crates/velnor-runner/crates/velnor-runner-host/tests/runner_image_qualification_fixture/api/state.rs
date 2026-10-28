// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageRole {
    Runner,
    Dind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Probe {
    RunnerListener,
    RunnerDocker,
    DindDocker,
    DindDockerd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContainerRole {
    RunnerListener,
    Dind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageFault {
    Id,
    Platform,
    Source,
    Reinspection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OuterMutation {
    None,
    Id,
    Root,
    Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApiFault {
    StallImageInspect,
    StallContainerCreate,
    LateContainerCreate,
    LateContainerCreateReplacement,
    DisconnectThenLateContainerCreate,
    NeverSettleContainerCreate,
    LoseContainerCreateResponse,
    StallContainerStart,
    StallContainerInspect,
    StallWait,
    StallLogs,
    StallExecCreate,
    StallExecStream,
    LoseVolumeCreateResponse,
    LateVolumeCreate,
    DisconnectThenLateVolumeCreate,
    NonzeroWait,
    ProbeStderr,
    MalformedOutput,
    OversizedOutput,
    UnfinishedExec,
    CleanupNotFound,
    CleanupServerError,
    CleanupDisconnect,
    CleanupStall,
    CleanupStillPresent,
    VolumeCleanupNotFound,
    VolumeCleanupServerError,
    VolumeCleanupDisconnect,
    VolumeCleanupStall,
    VolumeCleanupStillPresent,
    WrongOwnedConfig(ContainerRole),
    ForeignOwnerAtTrackedName,
    ReplacedContainerId,
    WrongRunnerSocketDaemon,
    RunnerSocketFailure,
}

pub(crate) struct ApiPlan {
    pub(crate) source_sha: String,
    pub(crate) runner_id: String,
    pub(crate) dind_id: String,
    pub(crate) runner_version: String,
    pub(crate) docker_version: String,
    pub(crate) image_fault: Option<(ImageRole, ImageFault)>,
    pub(crate) version_mismatch: Option<Probe>,
    pub(crate) readiness_empty_first: bool,
    pub(crate) outer_id: String,
    pub(crate) inner_id: String,
    pub(crate) outer_root: String,
    pub(crate) runner_socket_id: Option<String>,
    pub(crate) outer_mutation: OuterMutation,
    pub(crate) mutate_lock_on_first_image_inspect: Option<PathBuf>,
    pub(crate) mutate_source_on_first_image_inspect: Option<PathBuf>,
    pub(super) mutate_hidden_index_on_first_image_inspect: Option<HiddenIndexMutation>,
    pub(crate) fault: Option<ApiFault>,
    pub(crate) foreign_sentinel: bool,
}

pub(super) enum HiddenIndexMutation {
    SkipWorktree(PathBuf),
    AssumeUnchanged(PathBuf),
}

impl ApiPlan {
    pub(crate) fn new(source_sha: &str) -> Self {
        Self {
            source_sha: source_sha.to_owned(),
            runner_id: digest_id('a'),
            dind_id: digest_id('b'),
            runner_version: "2.338.0".to_owned(),
            docker_version: "29.9.1".to_owned(),
            image_fault: None,
            version_mismatch: None,
            readiness_empty_first: true,
            outer_id: "outer-daemon-id".to_owned(),
            inner_id: "inner-daemon-id".to_owned(),
            outer_root: "/var/lib/docker".to_owned(),
            runner_socket_id: None,
            outer_mutation: OuterMutation::None,
            mutate_lock_on_first_image_inspect: None,
            mutate_source_on_first_image_inspect: None,
            mutate_hidden_index_on_first_image_inspect: None,
            fault: None,
            foreign_sentinel: false,
        }
    }

    pub(crate) fn mutate_skip_worktree_on_first_image_inspect(&mut self, root: PathBuf) {
        self.mutate_hidden_index_on_first_image_inspect =
            Some(HiddenIndexMutation::SkipWorktree(root));
    }

    pub(crate) fn mutate_assume_unchanged_on_first_image_inspect(&mut self, root: PathBuf) {
        self.mutate_hidden_index_on_first_image_inspect =
            Some(HiddenIndexMutation::AssumeUnchanged(root));
    }
}

pub(crate) fn digest_id(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

#[derive(Clone, Debug)]
pub(crate) struct ContainerEvidence {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) role: String,
    pub(crate) labels: Value,
    pub(crate) image_id: String,
    pub(crate) host_config: Value,
    pub(crate) entrypoint: Value,
    pub(crate) command: Value,
}

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub(crate) images: Vec<String>,
    pub(crate) containers: Vec<ContainerEvidence>,
    pub(crate) volumes: Vec<String>,
    pub(crate) created: Vec<ContainerEvidence>,
    pub(crate) readiness_attempts: usize,
    pub(crate) empty_readiness_attempts: usize,
    pub(crate) image_inspections: usize,
    pub(crate) volume_creations: usize,
    pub(crate) api_requests: usize,
    pub(crate) fault_reached: bool,
    pub(crate) hidden_index_mutation: Option<Result<(), String>>,
    pub(crate) foreign_container_present: bool,
    pub(crate) foreign_volume_present: bool,
    pub(crate) outer_info_calls: usize,
    pub(crate) pending_mutations: usize,
    pub(crate) pending_identities: Vec<String>,
}

pub(super) struct FakeContainer {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) image_id: String,
    pub(super) role: String,
    pub(super) labels: Value,
    pub(super) host_config: Value,
    pub(super) entrypoint: Value,
    pub(super) command: Value,
    pub(super) running: bool,
    pub(super) exit_code: Option<i64>,
}

pub(super) struct FakeVolume {
    pub(super) name: String,
    pub(super) labels: Value,
}

pub(super) enum DeferredMutation {
    Container(FakeContainer),
    Volume(FakeVolume),
}

pub(super) struct FakeExec {
    pub(super) id: String,
    pub(super) container_id: String,
    pub(super) output: Vec<u8>,
    pub(super) running: bool,
    pub(super) exit_code: Option<i64>,
}

pub(super) struct State {
    pub(super) plan: ApiPlan,
    pub(super) images: HashSet<String>,
    pub(super) containers: HashMap<String, FakeContainer>,
    pub(super) volumes: HashMap<String, FakeVolume>,
    pub(super) execs: HashMap<String, FakeExec>,
    pub(super) created: Vec<ContainerEvidence>,
    pub(super) consumed_fault: bool,
    pub(super) next_id: u64,
    pub(super) next_exec: u64,
    pub(super) image_inspections: HashMap<String, usize>,
    pub(super) readiness_attempts: usize,
    pub(super) empty_readiness_attempts: usize,
    pub(super) outer_info_calls: usize,
    pub(super) volume_creations: usize,
    pub(super) api_requests: usize,
    pub(super) fault_reached: bool,
    pub(super) hidden_index_mutation: Option<Result<(), String>>,
    pub(super) pending_mutations: usize,
    pub(super) provisional_missing_responses: usize,
    pub(super) pending_identities: Vec<String>,
}

impl State {
    pub(super) fn new(plan: ApiPlan) -> Self {
        let images = HashSet::from([plan.runner_id.clone(), plan.dind_id.clone()]);
        let mut state = Self {
            plan,
            images,
            containers: HashMap::new(),
            volumes: HashMap::new(),
            execs: HashMap::new(),
            created: Vec::new(),
            consumed_fault: false,
            next_id: 0,
            next_exec: 0,
            image_inspections: HashMap::new(),
            readiness_attempts: 0,
            empty_readiness_attempts: 0,
            outer_info_calls: 0,
            volume_creations: 0,
            api_requests: 0,
            fault_reached: false,
            hidden_index_mutation: None,
            pending_mutations: 0,
            provisional_missing_responses: 0,
            pending_identities: Vec::new(),
        };
        if state.plan.foreign_sentinel {
            state.add_foreign_sentinel();
        }
        state
    }

    pub(super) fn take_fault(&mut self, fault: ApiFault) -> bool {
        if self.consumed_fault || self.plan.fault != Some(fault) {
            return false;
        }
        self.consumed_fault = true;
        self.fault_reached = true;
        true
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        Snapshot {
            images: self.images.iter().cloned().collect(),
            containers: self.containers.values().map(evidence).collect(),
            volumes: self.volumes.keys().cloned().collect(),
            created: self.created.clone(),
            readiness_attempts: self.readiness_attempts,
            empty_readiness_attempts: self.empty_readiness_attempts,
            image_inspections: self.image_inspections.values().sum(),
            volume_creations: self.volume_creations,
            api_requests: self.api_requests,
            fault_reached: self.fault_reached,
            hidden_index_mutation: self.hidden_index_mutation.clone(),
            foreign_container_present: self
                .containers
                .values()
                .any(|container| container.name == "foreign-sentinel"),
            foreign_volume_present: self.volumes.contains_key("foreign-sentinel"),
            outer_info_calls: self.outer_info_calls,
            pending_mutations: self.pending_mutations,
            pending_identities: self.pending_identities.clone(),
        }
    }

    pub(super) fn apply_deferred(&mut self, mutation: DeferredMutation) {
        let identity = match &mutation {
            DeferredMutation::Container(container) => format!("container {}", container.name),
            DeferredMutation::Volume(volume) => format!("volume {}", volume.name),
        };
        match mutation {
            DeferredMutation::Container(container) => {
                self.created.push(evidence(&container));
                self.containers.insert(container.id.clone(), container);
            }
            DeferredMutation::Volume(volume) => {
                self.volumes.insert(volume.name.clone(), volume);
                self.volume_creations += 1;
            }
        }
        self.pending_mutations = self.pending_mutations.saturating_sub(1);
        self.pending_identities
            .retain(|pending| pending != &identity);
    }

    pub(super) fn next_container_id(&mut self) -> String {
        self.next_id += 1;
        format!("{:064x}", self.next_id)
    }

    pub(super) fn next_exec_id(&mut self) -> String {
        self.next_exec += 1;
        format!("exec-{}", self.next_exec)
    }

    fn add_foreign_sentinel(&mut self) {
        self.containers.insert(
            "foreign-container-id".to_owned(),
            FakeContainer {
                id: "foreign-container-id".to_owned(),
                name: "foreign-sentinel".to_owned(),
                image_id: "foreign-image".to_owned(),
                role: "foreign".to_owned(),
                labels: serde_json::json!({
                    "io.velnor.runner-image-qualifier.run": "foreign-run",
                    "io.velnor.runner-image-qualifier.role": "foreign"
                }),
                host_config: serde_json::json!({}),
                entrypoint: Value::Null,
                command: Value::Null,
                running: true,
                exit_code: None,
            },
        );
        self.volumes.insert(
            "foreign-sentinel".to_owned(),
            FakeVolume {
                name: "foreign-sentinel".to_owned(),
                labels: serde_json::json!({
                    "io.velnor.runner-image-qualifier.run": "foreign-run",
                    "io.velnor.runner-image-qualifier.role": "foreign"
                }),
            },
        );
    }
}

fn evidence(container: &FakeContainer) -> ContainerEvidence {
    ContainerEvidence {
        id: container.id.clone(),
        name: container.name.clone(),
        role: container.role.clone(),
        labels: container.labels.clone(),
        image_id: container.image_id.clone(),
        host_config: container.host_config.clone(),
        entrypoint: container.entrypoint.clone(),
        command: container.command.clone(),
    }
}
