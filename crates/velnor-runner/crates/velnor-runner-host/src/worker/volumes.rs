// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::names::WorkerIdentityNames;
pub(crate) use super::names::WorkerVolumeRole;

use crate::docker_spec::Mount as PlannedMount;
use crate::error::HostError;
use velnor_runner_core::runner_work_path;

#[derive(Debug, Eq, PartialEq)]
struct WorkerVolume {
    name: String,
}

fn valid_worker(worker: &str) -> bool {
    !worker.is_empty()
        && worker
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

fn worker_volume(worker: &str, role: WorkerVolumeRole) -> Result<WorkerVolume, HostError> {
    let names = WorkerIdentityNames::from_private_volume(worker)?;
    Ok(WorkerVolume {
        name: names.volume_name(role),
    })
}

#[derive(Clone, Copy)]
enum ContainerKind {
    Runner,
    Dind,
}

pub(crate) fn mounts_for_runner(worker: &str) -> Result<Vec<PlannedMount>, HostError> {
    mounts_for(worker, ContainerKind::Runner, false)
}

pub(crate) fn mounts_for_runner_plan(worker: &str) -> Result<Vec<PlannedMount>, HostError> {
    mounts_for(worker, ContainerKind::Runner, true)
}

pub(crate) fn mounts_for_dind(worker: &str) -> Result<Vec<PlannedMount>, HostError> {
    mounts_for(worker, ContainerKind::Dind, false)
}

fn mounts_for(
    worker: &str,
    container: ContainerKind,
    runner_plan: bool,
) -> Result<Vec<PlannedMount>, HostError> {
    if !valid_worker(worker) {
        return Err(HostError::ForbiddenMount);
    }
    let mut mounts = Vec::new();
    for role in WorkerVolumeRole::ALL {
        if runner_plan && role == WorkerVolumeRole::Home {
            continue;
        }
        if let Some(target) = target(role, container) {
            let volume = worker_volume(worker, role)?;
            mounts.push(PlannedMount {
                source: format!("volume:{}", volume.name),
                target,
            });
        }
    }
    Ok(mounts)
}

fn target(role: WorkerVolumeRole, container: ContainerKind) -> Option<String> {
    match (role, container) {
        (WorkerVolumeRole::Socket, _) => Some("/run".to_owned()),
        (WorkerVolumeRole::Home, _) => Some("/home/runner".to_owned()),
        (WorkerVolumeRole::Work, _) => Some(runner_work_path()),
        (WorkerVolumeRole::DindData, ContainerKind::Dind) => Some("/var/lib/docker".to_owned()),
        (WorkerVolumeRole::DindData | WorkerVolumeRole::RunnerMetadata, ContainerKind::Runner) => {
            None
        }
        (WorkerVolumeRole::RunnerMetadata, ContainerKind::Dind) => None,
    }
}
