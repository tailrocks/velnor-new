//! Fixed storage role attachments.

use super::{
    StorageAttachmentContract, StorageObserver, StorageRole, attachment, attachment_with_subpath,
};

use StorageObserver::{Dind, Probe, Runner};

pub(super) const RUNNER_ROOT: [StorageAttachmentContract; 1] =
    [attachment(StorageRole::RunnerRoot, Runner, "/", true, None)];
pub(super) const RUNNER_HOME: [StorageAttachmentContract; 3] = [
    attachment(StorageRole::RunnerHome, Runner, "/home/runner", false, None),
    attachment(StorageRole::RunnerHome, Dind, "/home/runner", false, None),
    attachment_with_subpath(
        StorageRole::RunnerHome,
        Runner,
        "/opt/velnor/source-proof/inbox",
        false,
        None,
        Some("source-proof/inbox"),
    ),
];
pub(super) const RUNNER_WORK: [StorageAttachmentContract; 2] = [
    attachment(
        StorageRole::RunnerWork,
        Runner,
        "/home/runner/_work",
        false,
        None,
    ),
    attachment(
        StorageRole::RunnerWork,
        Dind,
        "/home/runner/_work",
        false,
        None,
    ),
];
pub(super) const RUNNER_TMP: [StorageAttachmentContract; 1] = [attachment(
    StorageRole::RunnerTmp,
    Runner,
    "/tmp",
    false,
    None,
)];
pub(super) const RUNNER_SHM: [StorageAttachmentContract; 1] = [attachment(
    StorageRole::RunnerShm,
    Runner,
    "/dev/shm",
    false,
    None,
)];
pub(super) const SHARED_RUN: [StorageAttachmentContract; 2] = [
    attachment(StorageRole::SharedRun, Runner, "/run", false, None),
    attachment(StorageRole::SharedRun, Dind, "/run", false, None),
];
pub(super) const DIND_ROOT: [StorageAttachmentContract; 1] =
    [attachment(StorageRole::DindRoot, Dind, "/", true, None)];
pub(super) const DIND_DATA: [StorageAttachmentContract; 1] = [attachment(
    StorageRole::DindData,
    Dind,
    "/var/lib/docker",
    false,
    None,
)];
pub(super) const DIND_TMP: [StorageAttachmentContract; 1] =
    [attachment(StorageRole::DindTmp, Dind, "/tmp", false, None)];
pub(super) const DIND_SHM: [StorageAttachmentContract; 1] = [attachment(
    StorageRole::DindShm,
    Dind,
    "/dev/shm",
    false,
    None,
)];
pub(super) const ENGINE_METADATA: [StorageAttachmentContract; 6] = [
    attachment(
        StorageRole::EngineMetadata,
        Probe,
        "/velnor/docker-root",
        true,
        Some("/var/lib/docker"),
    ),
    attachment_with_subpath(
        StorageRole::EngineMetadata,
        Runner,
        "/.velnor-runtime-identity",
        true,
        None,
        Some("runtime-identity"),
    ),
    attachment_with_subpath(
        StorageRole::EngineMetadata,
        Runner,
        "/opt/velnor/job-facts",
        true,
        None,
        Some("job-facts"),
    ),
    attachment_with_subpath(
        StorageRole::EngineMetadata,
        Runner,
        "/opt/velnor/source-proof/control",
        true,
        None,
        Some("source-proof/control"),
    ),
    attachment_with_subpath(
        StorageRole::EngineMetadata,
        Dind,
        "/.velnor-runtime-identity",
        true,
        None,
        Some("runtime-identity"),
    ),
    attachment_with_subpath(
        StorageRole::EngineMetadata,
        Dind,
        "/opt/velnor/job-facts",
        true,
        None,
        Some("job-facts"),
    ),
];
