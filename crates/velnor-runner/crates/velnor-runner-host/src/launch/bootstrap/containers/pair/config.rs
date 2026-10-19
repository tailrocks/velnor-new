use super::super::BootstrapContainerRole;
use super::PairCreate;
use crate::error::HostError;
use crate::journal::{StorageProvisioningPlan, StorageRole};
use crate::worker::{WorkerIdentityNames, WorkerVolumeRole};
use bollard::models::{
    ContainerCreateBody, HostConfig, HostConfigLogConfig, Mount, MountType, MountVolumeOptions,
};
use bollard::query_parameters::CreateContainerOptions;

fn storage_volume(names: &WorkerIdentityNames, role: StorageRole) -> Result<String, HostError> {
    let volume_role = match role {
        StorageRole::SharedRun => WorkerVolumeRole::Socket,
        StorageRole::RunnerHome => WorkerVolumeRole::Home,
        StorageRole::RunnerWork => WorkerVolumeRole::Work,
        StorageRole::DindData => WorkerVolumeRole::DindData,
        StorageRole::EngineMetadata => WorkerVolumeRole::RunnerMetadata,
        _ => return Err(HostError::Identity),
    };
    Ok(names.volume_name(volume_role))
}

fn plan_charge(plan: &StorageProvisioningPlan, role: StorageRole) -> Result<(u64, u64), HostError> {
    let mut matches = plan.roles().iter().filter(|entry| entry.role() == role);
    let value = matches.next().ok_or(HostError::Identity)?;
    if matches.next().is_some() || value.charge_bytes() == 0 || value.charge_inodes() == 0 {
        return Err(HostError::Identity);
    }
    Ok((value.charge_bytes(), value.charge_inodes()))
}

fn tmpfs_mounts(
    plan: &StorageProvisioningPlan,
    role: BootstrapContainerRole,
) -> Result<std::collections::HashMap<String, String>, HostError> {
    let roles: &[(StorageRole, &str)] = match role {
        BootstrapContainerRole::Runner => &[
            (StorageRole::RunnerTmp, "/tmp"),
            (StorageRole::RunnerShm, "/dev/shm"),
        ],
        BootstrapContainerRole::Dind => &[
            (StorageRole::DindTmp, "/tmp"),
            (StorageRole::DindShm, "/dev/shm"),
        ],
    };
    roles
        .iter()
        .map(|(storage_role, target)| {
            let (bytes, inodes) = plan_charge(plan, *storage_role)?;
            Ok(((*target).to_owned(), tmpfs_options(bytes, inodes)?))
        })
        .collect()
}

pub(in crate::launch::bootstrap::containers) fn tmpfs_options(
    bytes: u64,
    inodes: u64,
) -> Result<String, HostError> {
    if bytes == 0 || inodes == 0 {
        return Err(HostError::Identity);
    }
    Ok(format!("rw,nosuid,nodev,size={bytes},nr_inodes={inodes}"))
}

pub(in crate::launch::bootstrap::containers) fn limits_for(
    role: BootstrapContainerRole,
) -> (i64, bool) {
    match role {
        BootstrapContainerRole::Runner => (super::RUNNER_PIDS, false),
        BootstrapContainerRole::Dind => (super::DIND_PIDS, true),
    }
}

fn volume_mount(name: String, target: &str, read_only: bool) -> Mount {
    volume_subpath_mount(name, target, read_only, None)
}

fn volume_subpath_mount(
    name: String,
    target: &str,
    read_only: bool,
    subpath: Option<&str>,
) -> Mount {
    Mount {
        target: Some(target.to_owned()),
        source: Some(name),
        typ: Some(MountType::VOLUME),
        read_only: Some(read_only),
        volume_options: Some(MountVolumeOptions {
            no_copy: Some(true),
            subpath: subpath.map(str::to_owned),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(in crate::launch::bootstrap::containers) fn fixed_mounts(
    names: &WorkerIdentityNames,
    role: BootstrapContainerRole,
) -> Result<Vec<Mount>, HostError> {
    let mut mounts = Vec::new();
    match role {
        BootstrapContainerRole::Runner => {
            let home = storage_volume(names, StorageRole::RunnerHome)?;
            mounts.push(volume_mount(home.clone(), "/home/runner", false));
            mounts.push(volume_subpath_mount(
                home,
                "/opt/velnor/source-proof/inbox",
                false,
                Some("source-proof/inbox"),
            ));
            mounts.push(volume_mount(
                storage_volume(names, StorageRole::RunnerWork)?,
                "/home/runner/_work",
                false,
            ));
            mounts.push(volume_mount(
                storage_volume(names, StorageRole::SharedRun)?,
                "/run",
                false,
            ));
            let metadata = storage_volume(names, StorageRole::EngineMetadata)?;
            for (subpath, target) in [
                ("runtime-identity", "/.velnor-runtime-identity"),
                ("job-facts", "/opt/velnor/job-facts"),
                ("source-proof/control", "/opt/velnor/source-proof/control"),
            ] {
                mounts.push(volume_subpath_mount(
                    metadata.clone(),
                    target,
                    true,
                    Some(subpath),
                ));
            }
        }
        BootstrapContainerRole::Dind => {
            mounts.push(volume_mount(
                storage_volume(names, StorageRole::RunnerHome)?,
                "/home/runner",
                false,
            ));
            mounts.push(volume_mount(
                storage_volume(names, StorageRole::RunnerWork)?,
                "/home/runner/_work",
                false,
            ));
            mounts.push(volume_mount(
                storage_volume(names, StorageRole::SharedRun)?,
                "/run",
                false,
            ));
            mounts.push(volume_mount(
                storage_volume(names, StorageRole::DindData)?,
                "/var/lib/docker",
                false,
            ));
        }
    }
    Ok(mounts)
}

pub(in crate::launch::bootstrap::containers) fn create_body(
    name: String,
    image: String,
    plan: &StorageProvisioningPlan,
    names: &WorkerIdentityNames,
    connection_id: &str,
    role: BootstrapContainerRole,
    network_mode: String,
    budget: crate::worker::ResourceBudget,
) -> Result<PairCreate, HostError> {
    if plan.roles().len() != StorageRole::PHYSICAL.len()
        || plan.reservation_id() <= 0
        || plan.operation_id() <= 0
        || plan.engine_id().is_empty()
        || plan.docker_root_digest().len() != 64
        || connection_id.is_empty()
    {
        return Err(HostError::Identity);
    }
    let labels = container_labels(plan, connection_id, role);
    let host = host_config(plan, names, role, network_mode, budget)?;
    let open_stdin = role == BootstrapContainerRole::Runner;
    let config = ContainerCreateBody {
        image: Some(image),
        attach_stdin: Some(open_stdin),
        attach_stdout: Some(false),
        attach_stderr: Some(false),
        open_stdin: Some(open_stdin),
        stdin_once: Some(open_stdin),
        tty: Some(false),
        labels: Some(labels),
        host_config: Some(host),
        ..Default::default()
    };
    Ok(PairCreate {
        options: CreateContainerOptions {
            name: Some(name),
            platform: super::PLATFORM.to_owned(),
        },
        config,
    })
}

fn container_labels(
    plan: &StorageProvisioningPlan,
    connection_id: &str,
    role: BootstrapContainerRole,
) -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([
        ("io.velnor.managed".to_owned(), "true".to_owned()),
        (
            "io.velnor.reservation-id".to_owned(),
            plan.reservation_id().to_string(),
        ),
        (
            "io.velnor.operation-id".to_owned(),
            plan.operation_id().to_string(),
        ),
        (
            "io.velnor.connection-id".to_owned(),
            connection_id.to_owned(),
        ),
        (
            "io.velnor.role".to_owned(),
            match role {
                BootstrapContainerRole::Runner => "runner",
                BootstrapContainerRole::Dind => "dind",
            }
            .to_owned(),
        ),
    ])
}

fn host_config(
    plan: &StorageProvisioningPlan,
    names: &WorkerIdentityNames,
    role: BootstrapContainerRole,
    network_mode: String,
    budget: crate::worker::ResourceBudget,
) -> Result<HostConfig, HostError> {
    let limits = match role {
        BootstrapContainerRole::Runner => budget.runner(),
        BootstrapContainerRole::Dind => budget.dind(),
    };
    let (pids, privileged) = limits_for(role);
    Ok(HostConfig {
        mounts: Some(fixed_mounts(names, role)?),
        tmpfs: Some(tmpfs_mounts(plan, role)?),
        network_mode: Some(network_mode),
        readonly_rootfs: Some(true),
        privileged: Some(privileged),
        nano_cpus: Some(limits.nano_cpus),
        memory: Some(limits.memory_bytes),
        memory_swap: Some(limits.memory_bytes),
        pids_limit: Some(pids),
        log_config: Some(HostConfigLogConfig {
            typ: Some("json-file".to_owned()),
            config: Some(std::collections::HashMap::from([
                ("max-size".to_owned(), super::LOG_MAX_SIZE.to_owned()),
                ("max-file".to_owned(), super::LOG_MAX_FILES.to_owned()),
            ])),
        }),
        ..Default::default()
    })
}
