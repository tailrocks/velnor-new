use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType, MountVolumeOptions};
use bollard::query_parameters::CreateContainerOptions;

use crate::{error::HostError, launch::VerifiedWorkerImages};

use super::{
    super::{DockerResourceLimits, ResourceBudget},
    request_result::valid_operation_id,
};

const PLATFORM: &str = "linux/amd64";
const EXECUTABLE: &str = "/velnor/storage-init";
const HOME_TARGET: &str = "/velnor/init/home";
const WORK_TARGET: &str = "/velnor/init/work";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StorageInitializationImage {
    runtime_id: String,
    source_sha: String,
}

impl StorageInitializationImage {
    pub(crate) fn from_verified_release(images: &VerifiedWorkerImages) -> Result<Self, HostError> {
        let identity = images.runner();
        if images.platform() != PLATFORM
            || !sha256_digest(identity.runtime_id())
            || !lower_hex(images.source_sha(), 40)
        {
            return Err(HostError::Identity);
        }
        Ok(Self {
            runtime_id: identity.runtime_id().to_owned(),
            source_sha: images.source_sha().to_owned(),
        })
    }

    pub(crate) fn source_sha(&self) -> &str {
        &self.source_sha
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StorageInitializationLimits {
    docker: DockerResourceLimits,
    pids: i64,
}

impl StorageInitializationLimits {
    pub(crate) fn from_reserved_budget(
        budget: ResourceBudget,
        cpu_millicores: u64,
        memory_bytes: u64,
        pids: u64,
    ) -> Result<Self, HostError> {
        let runner = budget.runner();
        let runner_cpu_millicores =
            u64::try_from(runner.nano_cpus).map_err(|_| HostError::Config)? / 1_000_000;
        if cpu_millicores == 0
            || cpu_millicores > runner_cpu_millicores
            || memory_bytes < 6_291_456
            || memory_bytes > u64::try_from(runner.memory_bytes).map_err(|_| HostError::Config)?
            || pids == 0
        {
            return Err(HostError::Config);
        }
        let nano_cpus = cpu_millicores
            .checked_mul(1_000_000)
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(HostError::Config)?;
        Ok(Self {
            docker: DockerResourceLimits {
                nano_cpus,
                memory_bytes: i64::try_from(memory_bytes).map_err(|_| HostError::Config)?,
            },
            pids: i64::try_from(pids).map_err(|_| HostError::Config)?,
        })
    }
}

pub(crate) struct StorageInitializationCreate {
    pub(crate) options: CreateContainerOptions,
    pub(crate) config: ContainerCreateBody,
}

pub(crate) fn specification(
    operation_id: &str,
    home_volume: &str,
    work_volume: &str,
    image: &StorageInitializationImage,
    limits: StorageInitializationLimits,
) -> Result<StorageInitializationCreate, HostError> {
    if !valid_operation_id(operation_id)
        || !valid_volume_name(home_volume)
        || !valid_volume_name(work_volume)
        || home_volume == work_volume
    {
        return Err(HostError::Identity);
    }
    let name = format!("velnor-init-{operation_id}");
    let labels = [
        ("velnor.role", "storage-initializer"),
        ("velnor.operation", operation_id),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    Ok(StorageInitializationCreate {
        options: CreateContainerOptions {
            name: Some(name),
            platform: PLATFORM.to_owned(),
        },
        config: ContainerCreateBody {
            image: Some(image.runtime_id.clone()),
            user: Some("0:0".to_owned()),
            entrypoint: Some(vec![EXECUTABLE.to_owned()]),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            open_stdin: Some(true),
            stdin_once: Some(true),
            tty: Some(false),
            network_disabled: Some(true),
            labels: Some(labels),
            host_config: Some(HostConfig {
                nano_cpus: Some(limits.docker.nano_cpus),
                memory: Some(limits.docker.memory_bytes),
                memory_swap: Some(limits.docker.memory_bytes),
                pids_limit: Some(limits.pids),
                network_mode: Some("none".to_owned()),
                cap_drop: Some(vec!["ALL".to_owned()]),
                cap_add: Some(vec!["CHOWN".to_owned(), "FOWNER".to_owned()]),
                privileged: Some(false),
                readonly_rootfs: Some(true),
                security_opt: Some(vec!["no-new-privileges:true".to_owned()]),
                mounts: Some(vec![
                    volume_mount(home_volume, HOME_TARGET),
                    volume_mount(work_volume, WORK_TARGET),
                ]),
                ..HostConfig::default()
            }),
            ..ContainerCreateBody::default()
        },
    })
}

fn volume_mount(source: &str, target: &str) -> Mount {
    Mount {
        target: Some(target.to_owned()),
        source: Some(source.to_owned()),
        typ: Some(MountType::VOLUME),
        read_only: Some(false),
        volume_options: Some(MountVolumeOptions {
            no_copy: Some(true),
            ..MountVolumeOptions::default()
        }),
        ..Mount::default()
    }
}

fn valid_volume_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

fn sha256_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| lower_hex(digest, 64))
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
pub(super) fn test_image() -> StorageInitializationImage {
    StorageInitializationImage {
        runtime_id: format!("sha256:{}", "a".repeat(64)),
        source_sha: "b".repeat(40),
    }
}
