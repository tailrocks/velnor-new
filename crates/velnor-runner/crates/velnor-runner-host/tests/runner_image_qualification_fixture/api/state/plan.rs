// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use super::digest_id;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageRole {
    Runner,
    Dind,
    ResourceProbe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Probe {
    RunnerListener,
    RunnerDocker,
    DindDocker,
    DindDockerd,
    RunnerBuildx,
    RunnerCompose,
    RunnerNode,
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
    Kind,
    User,
    MissingUser,
    Entrypoint,
    MissingEntrypoint,
    WorkingDir,
    DescriptorDigest,
    DescriptorPlatform,
    DescriptorReinspection,
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
    OwnedUnsettledDindCreate,
    OwnedUnsettledDindRemovalFailure,
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
    pub(crate) probe_id: String,
    pub(crate) runner_version: String,
    pub(crate) docker_version: String,
    pub(crate) buildx_version: String,
    pub(crate) compose_version: String,
    pub(crate) node_version: String,
    pub(crate) image_fault: Option<(ImageRole, ImageFault)>,
    pub(crate) malformed_resource_probe: bool,
    pub(crate) manifest_store: Option<()>,
    pub(crate) version_mismatch: Option<Probe>,
    pub(crate) readiness_empty_first: bool,
    pub(crate) outer_id: String,
    pub(crate) inner_id: String,
    pub(crate) inner_root: String,
    pub(crate) inner_driver: String,
    pub(crate) outer_root: String,
    pub(crate) runner_socket_id: Option<String>,
    pub(crate) outer_mutation: OuterMutation,
    pub(crate) mutate_lock_on_first_image_inspect: Option<PathBuf>,
    pub(crate) mutate_source_on_first_image_inspect: Option<PathBuf>,
    pub(crate) mutate_hidden_index_on_first_image_inspect: Option<HiddenIndexMutation>,
    pub(crate) fault: Option<ApiFault>,
    pub(crate) foreign_sentinel: bool,
    pub(crate) recovery_image: Option<String>,
}

pub(crate) enum HiddenIndexMutation {
    SkipWorktree(PathBuf),
    AssumeUnchanged(PathBuf),
}

impl ApiPlan {
    pub(crate) fn new(source_sha: &str) -> Self {
        Self {
            source_sha: source_sha.to_owned(),
            runner_id: digest_id('a'),
            dind_id: digest_id('b'),
            probe_id: digest_id('c'),
            runner_version: "2.338.0".to_owned(),
            docker_version: "29.9.1".to_owned(),
            buildx_version: "0.38.0".to_owned(),
            compose_version: "5.6.0".to_owned(),
            node_version: "26.12.0".to_owned(),
            image_fault: None,
            malformed_resource_probe: false,
            manifest_store: None,
            version_mismatch: None,
            readiness_empty_first: true,
            outer_id: "outer-daemon-id".to_owned(),
            inner_id: "inner-daemon-id".to_owned(),
            inner_root: "/var/lib/docker".to_owned(),
            inner_driver: "vfs".to_owned(),
            outer_root: "/var/lib/docker".to_owned(),
            runner_socket_id: None,
            outer_mutation: OuterMutation::None,
            mutate_lock_on_first_image_inspect: None,
            mutate_source_on_first_image_inspect: None,
            mutate_hidden_index_on_first_image_inspect: None,
            fault: None,
            foreign_sentinel: false,
            recovery_image: None,
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
