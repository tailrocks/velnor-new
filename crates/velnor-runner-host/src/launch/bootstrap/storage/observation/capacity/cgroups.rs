// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_resource_probe::worker_binding::{
    AttachmentRole, CgroupNodeObservation, OwnedCgroupObservation, OwnedContainerCgroupObservation,
    SelfLeafLimits, SelfStorageObservation, StorageAttachmentObservation, WorkerRole,
};

use crate::{
    error::HostError,
    launch::bootstrap::{BootstrapContainerRole, ObservedContainerCgroup},
};

const TMPFS_MAGIC: u64 = 0x0102_1994;
const SHARED_TMPFS_ROLES: [AttachmentRole; 3] = [
    AttachmentRole::RunnerHome,
    AttachmentRole::RunnerWork,
    AttachmentRole::SharedRun,
];

pub fn observed_cgroups(
    runner: &SelfStorageObservation,
    dind: &SelfStorageObservation,
    owned: &OwnedCgroupObservation,
    runner_id: &str,
    dind_id: &str,
) -> Result<Vec<ObservedContainerCgroup>, HostError> {
    if runner_id.is_empty()
        || dind_id.is_empty()
        || runner_id == dind_id
        || runner.role != WorkerRole::Runner
        || dind.role != WorkerRole::Dind
        || runner.container_id != runner_id
        || dind.container_id != dind_id
    {
        return Err(HostError::Identity);
    }
    let owned_runner = find_owned_container(owned, WorkerRole::Runner, runner_id)?;
    let owned_dind = find_owned_container(owned, WorkerRole::Dind, dind_id)?;
    if owned_runner.init.cgroup_path.is_empty()
        || owned_dind.init.cgroup_path.is_empty()
        || owned_runner.init.cgroup_path == owned_dind.init.cgroup_path
        || owned_dind
            .hierarchy
            .iter()
            .any(|node| node.path == owned_runner.init.cgroup_path)
        || owned_runner
            .hierarchy
            .iter()
            .any(|node| node.path == owned_dind.init.cgroup_path)
    {
        return Err(HostError::Identity);
    }
    validate_hierarchy_nodes(&owned_runner.hierarchy)?;
    validate_hierarchy_nodes(&owned_dind.hierarchy)?;
    let runner_exclusive = exclusive_nodes(&owned_runner.hierarchy, &owned_dind.hierarchy)?;
    let dind_exclusive = exclusive_nodes(&owned_dind.hierarchy, &owned_runner.hierarchy)?;
    let runner_cgroup = observed_exclusive_cgroup(
        runner,
        owned_runner,
        &runner_exclusive,
        BootstrapContainerRole::Runner,
        runner_id,
    )?;
    let dind_cgroup = observed_exclusive_cgroup(
        dind,
        owned_dind,
        &dind_exclusive,
        BootstrapContainerRole::Dind,
        dind_id,
    )?;
    let shared_tmpfs_bytes = shared_tmpfs_overlap(runner, dind)?;
    validate_shared_ancestors(
        &owned_runner.hierarchy,
        &owned_dind.hierarchy,
        &runner_cgroup,
        &dind_cgroup,
        shared_tmpfs_bytes,
    )?;
    Ok(vec![runner_cgroup, dind_cgroup])
}

fn find_owned_container<'a>(
    owned: &'a OwnedCgroupObservation,
    role: WorkerRole,
    container_id: &str,
) -> Result<&'a OwnedContainerCgroupObservation, HostError> {
    let mut matches = owned
        .containers
        .iter()
        .filter(|row| row.role == role && row.container_id == container_id);
    let owned_container = matches.next().ok_or(HostError::Identity)?;
    if matches.next().is_some() {
        return Err(HostError::Identity);
    }
    Ok(owned_container)
}

fn validate_hierarchy_nodes(nodes: &[CgroupNodeObservation]) -> Result<(), HostError> {
    for (index, node) in nodes.iter().enumerate() {
        if node.path.is_empty()
            || nodes[..index].iter().any(|prior| prior.path == node.path)
            || node.memory_max_bytes == Some(0)
            || node.pids_max == Some(0)
            || node.cpu_period_micros == Some(0)
            || node.cpu_quota_micros == Some(0)
            || (node.cpu_quota_micros.is_some() && node.cpu_period_micros.is_none())
            || node
                .memory_max_bytes
                .zip(node.memory_current_bytes)
                .is_some_and(|(max, cur)| cur > max)
            || node
                .pids_max
                .zip(node.pids_current)
                .is_some_and(|(max, cur)| cur > max)
        {
            return Err(HostError::Identity);
        }
    }
    Ok(())
}

fn exclusive_nodes<'a>(
    own: &'a [CgroupNodeObservation],
    sibling: &[CgroupNodeObservation],
) -> Result<Vec<&'a CgroupNodeObservation>, HostError> {
    let exclusive = own
        .iter()
        .filter(|node| !sibling.iter().any(|other| other.path == node.path))
        .collect::<Vec<_>>();
    if !own.is_empty() && exclusive.is_empty() {
        return Err(HostError::Identity);
    }
    Ok(exclusive)
}

fn observed_exclusive_cgroup(
    observation: &SelfStorageObservation,
    owned_container: &OwnedContainerCgroupObservation,
    exclusive: &[&CgroupNodeObservation],
    role: BootstrapContainerRole,
    container_id: &str,
) -> Result<ObservedContainerCgroup, HostError> {
    let limits = &observation.self_leaf_limits;
    if limits.cgroup_path.is_empty()
        || limits.cpu_period_micros == 0
        || limits.cpu_quota_micros == Some(0)
    {
        return Err(HostError::Identity);
    }
    let exclusive_memory = exclusive
        .iter()
        .filter_map(|node| node.memory_max_bytes)
        .min();
    let exclusive_pids = exclusive.iter().filter_map(|node| node.pids_max).min();
    let exclusive_cpu = exclusive_cpu_limit(exclusive);
    if limits
        .memory_max_bytes
        .zip(exclusive_memory)
        .is_some_and(|(leaf, anc)| anc < leaf)
        || limits
            .pids_max
            .zip(exclusive_pids)
            .is_some_and(|(leaf, anc)| anc < leaf)
    {
        return Err(HostError::Identity);
    }
    if let (Some(leaf_q), Some((anc_q, anc_p))) = (limits.cpu_quota_micros, exclusive_cpu)
        && !cpu_ratio_at_least((anc_q, anc_p), (leaf_q, limits.cpu_period_micros))
    {
        return Err(HostError::Identity);
    }
    let memory_max_bytes = exclusive_memory
        .into_iter()
        .chain(limits.memory_max_bytes)
        .min();
    let pids_max = exclusive_pids.into_iter().chain(limits.pids_max).min();
    let (cpu_quota_micros, cpu_period_micros) = effective_cpu_limit(exclusive_cpu, limits)?;
    if memory_max_bytes.is_none_or(|value| value == 0 || value < limits.memory_current_bytes)
        || cpu_quota_micros.is_none_or(|value| value == 0)
        || cpu_period_micros == 0
        || pids_max.is_none_or(|value| value == 0 || limits.pids_current > value)
    {
        return Err(HostError::Identity);
    }
    Ok(ObservedContainerCgroup {
        role,
        container_id: container_id.to_owned(),
        path: owned_container.init.cgroup_path.clone(),
        memory_max_bytes,
        memory_current_bytes: limits.memory_current_bytes,
        cpu_quota_micros,
        cpu_period_micros,
        pids_max,
    })
}

fn exclusive_cpu_limit(exclusive: &[&CgroupNodeObservation]) -> Option<(u64, u64)> {
    let mut best: Option<(u64, u64)> = None;
    for node in exclusive {
        if let (Some(quota), Some(period)) = (node.cpu_quota_micros, node.cpu_period_micros) {
            best = Some(tightest_cpu_pair(best, (quota, period)));
        }
    }
    best
}

fn effective_cpu_limit(
    exclusive_cpu: Option<(u64, u64)>,
    limits: &SelfLeafLimits,
) -> Result<(Option<u64>, u64), HostError> {
    let mut best = exclusive_cpu;
    if let Some(quota) = limits.cpu_quota_micros {
        best = Some(tightest_cpu_pair(best, (quota, limits.cpu_period_micros)));
    }
    let (quota, period) = best.ok_or(HostError::Identity)?;
    Ok((Some(quota), period))
}

fn tightest_cpu_pair(current: Option<(u64, u64)>, candidate: (u64, u64)) -> (u64, u64) {
    match current {
        Some(existing) if cpu_ratio_at_least(candidate, existing) => existing,
        _ => candidate,
    }
}

fn cpu_ratio_at_least(left: (u64, u64), right: (u64, u64)) -> bool {
    u128::from(left.0) * u128::from(right.1) >= u128::from(right.0) * u128::from(left.1)
}

fn shared_tmpfs_overlap(
    runner: &SelfStorageObservation,
    dind: &SelfStorageObservation,
) -> Result<u64, HostError> {
    let mut overlap = 0_u64;
    for role in SHARED_TMPFS_ROLES {
        let runner_att = unique_attachment(&runner.attachments, role)?;
        let dind_att = unique_attachment(&dind.attachments, role)?;
        if runner_att.filesystem_type_magic != TMPFS_MAGIC
            || runner_att.filesystem_type_name != "tmpfs"
            || runner_att.total_bytes == 0
            || runner_att.total_bytes != dind_att.total_bytes
            || runner_att.filesystem_type_magic != dind_att.filesystem_type_magic
            || runner_att.filesystem_type_name != dind_att.filesystem_type_name
            || runner_att.mount_device_major != dind_att.mount_device_major
            || runner_att.mount_device_minor != dind_att.mount_device_minor
            || runner_att.statx_device_major != dind_att.statx_device_major
            || runner_att.statx_device_minor != dind_att.statx_device_minor
            || runner_att.filesystem_id != dind_att.filesystem_id
            || runner_att.statx_inode != dind_att.statx_inode
        {
            return Err(HostError::Identity);
        }
        overlap = overlap
            .checked_add(runner_att.total_bytes)
            .ok_or(HostError::Identity)?;
    }
    Ok(overlap)
}

fn unique_attachment(
    attachments: &[StorageAttachmentObservation],
    role: AttachmentRole,
) -> Result<&StorageAttachmentObservation, HostError> {
    let mut matches = attachments.iter().filter(|item| item.role == role);
    let attachment = matches.next().ok_or(HostError::Identity)?;
    if matches.next().is_some() {
        return Err(HostError::Identity);
    }
    Ok(attachment)
}

fn validate_shared_ancestors(
    runner_nodes: &[CgroupNodeObservation],
    dind_nodes: &[CgroupNodeObservation],
    runner_cgroup: &ObservedContainerCgroup,
    dind_cgroup: &ObservedContainerCgroup,
    shared_tmpfs_bytes: u64,
) -> Result<(), HostError> {
    let runner_memory = runner_cgroup.memory_max_bytes.ok_or(HostError::Identity)?;
    let dind_memory = dind_cgroup.memory_max_bytes.ok_or(HostError::Identity)?;
    if runner_memory <= shared_tmpfs_bytes || dind_memory <= shared_tmpfs_bytes {
        return Err(HostError::Identity);
    }
    let pair_memory = runner_memory
        .checked_add(dind_memory)
        .and_then(|sum| sum.checked_sub(shared_tmpfs_bytes))
        .ok_or(HostError::Identity)?;
    let pair_pids = runner_cgroup
        .pids_max
        .zip(dind_cgroup.pids_max)
        .and_then(|(left, right)| left.checked_add(right))
        .ok_or(HostError::Identity)?;
    let runner_cpu = (
        runner_cgroup.cpu_quota_micros.ok_or(HostError::Identity)?,
        runner_cgroup.cpu_period_micros,
    );
    let dind_cpu = (
        dind_cgroup.cpu_quota_micros.ok_or(HostError::Identity)?,
        dind_cgroup.cpu_period_micros,
    );
    for runner_node in runner_nodes {
        let Some(dind_node) = dind_nodes
            .iter()
            .find(|other| other.path == runner_node.path)
        else {
            continue;
        };
        if runner_node.root != dind_node.root
            || runner_node.memory_max_bytes != dind_node.memory_max_bytes
            || runner_node.pids_max != dind_node.pids_max
            || runner_node.cpu_quota_micros != dind_node.cpu_quota_micros
            || runner_node.cpu_period_micros != dind_node.cpu_period_micros
            || runner_node
                .memory_max_bytes
                .is_some_and(|shared| shared < pair_memory)
            || runner_node
                .pids_max
                .is_some_and(|shared| shared < pair_pids)
        {
            return Err(HostError::Identity);
        }
        if let (Some(shared_q), Some(shared_p)) =
            (runner_node.cpu_quota_micros, runner_node.cpu_period_micros)
            && !shared_cpu_fits_pair((shared_q, shared_p), runner_cpu, dind_cpu)?
        {
            return Err(HostError::Identity);
        }
    }
    Ok(())
}

fn shared_cpu_fits_pair(
    shared: (u64, u64),
    runner: (u64, u64),
    dind: (u64, u64),
) -> Result<bool, HostError> {
    let gcd = gcd_u128(u128::from(runner.1), u128::from(dind.1));
    let r_scale = u128::from(dind.1) / gcd;
    let d_scale = u128::from(runner.1) / gcd;
    let pair_period = (u128::from(runner.1) / gcd)
        .checked_mul(u128::from(dind.1))
        .ok_or(HostError::Identity)?;
    let pair_quota = u128::from(runner.0)
        .checked_mul(r_scale)
        .and_then(|left| {
            u128::from(dind.0)
                .checked_mul(d_scale)
                .and_then(|right| left.checked_add(right))
        })
        .ok_or(HostError::Identity)?;
    let pair_gcd = gcd_u128(pair_quota, pair_period);
    let shared_gcd = gcd_u128(u128::from(shared.0), u128::from(shared.1));
    let lhs = (u128::from(shared.0) / shared_gcd)
        .checked_mul(pair_period / pair_gcd)
        .ok_or(HostError::Identity)?;
    let rhs = (pair_quota / pair_gcd)
        .checked_mul(u128::from(shared.1) / shared_gcd)
        .ok_or(HostError::Identity)?;
    Ok(lhs >= rhs)
}

const fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    if a == 0 { 1 } else { a }
}
