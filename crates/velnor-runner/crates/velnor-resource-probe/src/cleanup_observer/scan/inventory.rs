// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    time::Instant,
};

use super::{
    super::CleanupObserverRequest,
    helpers::{
        add_issue, expired, io_vanished, linux_device, live_process_namespace, namespace_identity,
        namespace_link, numeric_name, observe_descriptor, process_start_ticks, read_bounded,
        vanished,
    },
    mounts::{check_owned_mounts, check_tmpfs_mounts, read_task_mountinfo},
};
use crate::cleanup_observer_wire::{
    descriptor_has_owned_tmpfs, observer_outside_pair_namespaces, observer_runtime_tmpfs_devices,
    owned_mountinfo_projection,
};

const PROC: &str = "/proc";
use crate::mountinfo_io::MOUNTINFO_LIMIT;
const MAX_PROCESSES: usize = 4096;
const MAX_TASKS: usize = 16_384;
const MAX_FDS_PER_TASK: usize = 4096;
const MAX_FDS: usize = 65_536;
const MAX_NAMESPACES: usize = 4096;

pub(super) struct Inventory {
    pub issues: Vec<String>,
    pub task_count: u32,
    pub descriptor_count: u32,
    pub namespace_count: u32,
    pub owned_reference_count: u32,
    namespaces: BTreeSet<([u32; 2], u64)>,
}

struct ScanContext<'a> {
    request: &'a CleanupObserverRequest,
    started: Instant,
    namespace_ids: [([u32; 2], u64); 2],
    tmpfs_devices: HashSet<u64>,
    observer_namespace: Option<([u32; 2], u64)>,
    observer_in_pair_namespace: bool,
    observer_tmpfs_devices: HashSet<u64>,
}

pub(super) fn scan(request: &CleanupObserverRequest, started: Instant) -> Inventory {
    let mut inventory = Inventory {
        issues: Vec::new(),
        task_count: 0,
        descriptor_count: 0,
        namespace_count: 0,
        owned_reference_count: 0,
        namespaces: BTreeSet::new(),
    };
    let context = context(request, started, &mut inventory.issues);
    if context.observer_in_pair_namespace {
        add_issue(&mut inventory.issues, "observer_in_pair_namespace");
    }
    let Ok(entries) = fs::read_dir(PROC) else {
        add_issue(&mut inventory.issues, "proc_list_unreadable");
        return inventory;
    };
    let mut process_count = 0;
    for entry in entries {
        if expired(started) {
            add_issue(&mut inventory.issues, "inventory_deadline");
            break;
        }
        let Ok(entry) = entry else {
            add_issue(&mut inventory.issues, "proc_entry_unreadable");
            continue;
        };
        let Some(pid) = numeric_name(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        process_count += 1;
        if process_count > MAX_PROCESSES {
            add_issue(&mut inventory.issues, "process_limit");
            break;
        }
        scan_process(pid, &context, &mut inventory);
    }
    inventory.namespace_count = u32::try_from(inventory.namespaces.len()).unwrap_or(u32::MAX);
    inventory
}

fn context<'a>(
    request: &'a CleanupObserverRequest,
    started: Instant,
    issues: &mut Vec<String>,
) -> ScanContext<'a> {
    let binding = &request.binding;
    let mut protected_namespaces = Vec::new();
    let mut identities_readable = true;
    for process in [&binding.daemon, &binding.runner, &binding.dind] {
        match live_process_namespace(process) {
            Ok(Some(namespace)) => protected_namespaces.push(namespace),
            Ok(None) => {}
            Err(_) => {
                identities_readable = false;
                add_issue(issues, "observer_identity_unreadable");
            }
        }
    }
    let own_namespace = namespace_identity("/proc/self/ns/mnt").ok();
    let outside_namespace = observer_outside_pair_namespaces(own_namespace, &protected_namespaces);
    let observer_namespace = outside_namespace.filter(|_| identities_readable);
    let self_mountinfo = read_bounded(
        "/proc/self/mountinfo",
        MOUNTINFO_LIMIT,
        "cleanup_self_mountinfo",
    )
    .unwrap_or_default();
    let observer_tmpfs_devices =
        observer_runtime_tmpfs_devices(observer_namespace, &self_mountinfo);
    ScanContext {
        request,
        started,
        namespace_ids: [
            (
                binding.runner.namespace_device,
                binding.runner.namespace_inode,
            ),
            (binding.dind.namespace_device, binding.dind.namespace_inode),
        ],
        tmpfs_devices: binding
            .owned_tmpfs_devices
            .iter()
            .map(|device| linux_device(u64::from(device[0]), u64::from(device[1])))
            .collect(),
        observer_namespace,
        observer_in_pair_namespace: own_namespace.is_some() && outside_namespace.is_none(),
        observer_tmpfs_devices,
    }
}

fn scan_process(pid: u32, context: &ScanContext<'_>, inventory: &mut Inventory) {
    let task_dir = format!("{PROC}/{pid}/task");
    let tasks = match fs::read_dir(task_dir) {
        Ok(tasks) => tasks,
        Err(error) if io_vanished(&error) => return,
        Err(_) => {
            add_issue(&mut inventory.issues, "task_list_unreadable");
            return;
        }
    };
    for task in tasks {
        if expired(context.started) {
            add_issue(&mut inventory.issues, "inventory_deadline");
            break;
        }
        let Ok(task) = task else {
            add_issue(&mut inventory.issues, "task_entry_unreadable");
            continue;
        };
        let Some(tid) = numeric_name(&task.file_name().to_string_lossy()) else {
            add_issue(&mut inventory.issues, "task_name_invalid");
            continue;
        };
        inventory.task_count = inventory.task_count.saturating_add(1);
        if usize::try_from(inventory.task_count).unwrap_or(usize::MAX) > MAX_TASKS {
            add_issue(&mut inventory.issues, "task_limit");
            break;
        }
        scan_task(pid, tid, context, inventory);
    }
}

fn scan_task(pid: u32, tid: u32, context: &ScanContext<'_>, inventory: &mut Inventory) {
    let stat_path = format!("{PROC}/{pid}/task/{tid}/stat");
    let start_before = match process_start_ticks(&stat_path, tid) {
        Ok(ticks) => ticks,
        Err(error) if vanished(&error) => return,
        Err(_) => {
            add_issue(&mut inventory.issues, "task_start_unreadable");
            return;
        }
    };
    let namespace_path = format!("{PROC}/{pid}/task/{tid}/ns/mnt");
    let namespace = match namespace_identity(&namespace_path) {
        Ok(namespace) => namespace,
        Err(error) if vanished(&error) => return,
        Err(_) => {
            add_issue(&mut inventory.issues, "task_namespace_unreadable");
            return;
        }
    };
    let is_observer_task = context.observer_namespace == Some(namespace);
    if context.namespace_ids.contains(&namespace) && !is_observer_task {
        inventory.owned_reference_count = inventory.owned_reference_count.saturating_add(1);
    }
    let new_namespace = inventory.namespaces.insert(namespace);
    if new_namespace {
        if inventory.namespaces.len() > MAX_NAMESPACES {
            add_issue(&mut inventory.issues, "namespace_limit");
        } else {
            scan_task_mounts(pid, tid, context, inventory);
        }
    }
    scan_descriptors(pid, tid, is_observer_task, context, inventory);
    match process_start_ticks(&stat_path, tid) {
        Ok(value) if value == start_before => {}
        Err(error) if vanished(&error) => {}
        _ => add_issue(&mut inventory.issues, "task_identity_changed"),
    }
}

fn scan_task_mounts(pid: u32, tid: u32, context: &ScanContext<'_>, inventory: &mut Inventory) {
    let Some(bytes) = read_task_mountinfo(pid, tid, &mut inventory.issues) else {
        return;
    };
    check_owned_mounts(
        pid,
        tid,
        &bytes,
        context.request,
        &context.observer_tmpfs_devices,
        &mut inventory.owned_reference_count,
        &mut inventory.issues,
    );
    check_tmpfs_mounts(
        pid,
        &bytes,
        (&context.tmpfs_devices, &context.observer_tmpfs_devices),
        &context.namespace_ids,
        &mut inventory.owned_reference_count,
        &mut inventory.issues,
    );
    if let Some(after) = read_task_mountinfo(pid, tid, &mut inventory.issues)
        && owned_mountinfo_projection(&after, &context.request.binding)
            != owned_mountinfo_projection(&bytes, &context.request.binding)
    {
        add_issue(&mut inventory.issues, "task_mountinfo_changed");
    }
}

fn scan_descriptors(
    pid: u32,
    tid: u32,
    is_observer_task: bool,
    context: &ScanContext<'_>,
    inventory: &mut Inventory,
) {
    let fd_path = format!("{PROC}/{pid}/task/{tid}/fd");
    let fds = match fs::read_dir(fd_path) {
        Ok(fds) => fds,
        Err(error) if io_vanished(&error) => return,
        Err(_) => {
            add_issue(&mut inventory.issues, "task_fd_list_unreadable");
            return;
        }
    };
    let mut per_task_fds = 0;
    for fd in fds {
        if expired(context.started) {
            add_issue(&mut inventory.issues, "inventory_deadline");
            break;
        }
        let Ok(fd) = fd else {
            add_issue(&mut inventory.issues, "task_fd_entry_unreadable");
            continue;
        };
        per_task_fds += 1;
        inventory.descriptor_count = inventory.descriptor_count.saturating_add(1);
        if per_task_fds > MAX_FDS_PER_TASK
            || usize::try_from(inventory.descriptor_count).unwrap_or(usize::MAX) > MAX_FDS
        {
            add_issue(&mut inventory.issues, "descriptor_limit");
            break;
        }
        observe_task_descriptor(&fd.path(), is_observer_task, context, inventory);
    }
}

fn observe_task_descriptor(
    path: &std::path::Path,
    is_observer_task: bool,
    context: &ScanContext<'_>,
    inventory: &mut Inventory,
) {
    let observation = match observe_descriptor(path) {
        Ok(observation) => observation,
        Err(error) if vanished(&error) => return,
        Err(_) => {
            add_issue(&mut inventory.issues, "task_fd_observation_incomplete");
            return;
        }
    };
    let namespace_reference = !is_observer_task
        && namespace_link(&observation.target).is_some_and(|inode| {
            observation.filesystem_magic == 0x6e73_6673
                && context
                    .namespace_ids
                    .iter()
                    .any(|(device, expected_inode)| {
                        *expected_inode == inode
                            && *expected_inode == observation.inode
                            && *device == observation.device
                    })
        });
    let tmpfs_reference = descriptor_has_owned_tmpfs(
        observation.filesystem_magic,
        observation.device,
        &observation.target,
        &context.tmpfs_devices,
        &context.observer_tmpfs_devices,
    );
    if namespace_reference || tmpfs_reference {
        inventory.owned_reference_count = inventory.owned_reference_count.saturating_add(1);
    }
}
