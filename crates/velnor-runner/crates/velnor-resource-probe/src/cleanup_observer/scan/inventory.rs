use std::{
    collections::{BTreeSet, HashSet},
    fs,
    time::Instant,
};

use super::{
    super::CleanupObserverRequest,
    helpers::{
        add_issue, expired, linux_device, namespace_identity, namespace_link, numeric_name,
        observe_descriptor, process_start_ticks, tmpfs_magic,
    },
    mounts::{check_owned_mounts, check_tmpfs_mounts, read_task_mountinfo},
};

const PROC: &str = "/proc";
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
    let context = context(request, started);
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

fn context(request: &CleanupObserverRequest, started: Instant) -> ScanContext<'_> {
    let binding = &request.binding;
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
    }
}

fn scan_process(pid: u32, context: &ScanContext<'_>, inventory: &mut Inventory) {
    let task_dir = format!("{PROC}/{pid}/task");
    let Ok(tasks) = fs::read_dir(task_dir) else {
        add_issue(&mut inventory.issues, "task_list_unreadable");
        return;
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
    let Ok(start_before) = process_start_ticks(&stat_path, tid) else {
        add_issue(&mut inventory.issues, "task_start_unreadable");
        return;
    };
    let namespace_path = format!("{PROC}/{pid}/task/{tid}/ns/mnt");
    let Ok(namespace) = namespace_identity(&namespace_path) else {
        add_issue(&mut inventory.issues, "task_namespace_unreadable");
        return;
    };
    if context.namespace_ids.contains(&namespace) {
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
    scan_descriptors(pid, tid, context, inventory);
    if !matches!(process_start_ticks(&stat_path, tid), Ok(value) if value == start_before) {
        add_issue(&mut inventory.issues, "task_identity_changed");
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
        &mut inventory.owned_reference_count,
        &mut inventory.issues,
    );
    check_tmpfs_mounts(
        pid,
        &bytes,
        &context.tmpfs_devices,
        &context.namespace_ids,
        &mut inventory.owned_reference_count,
        &mut inventory.issues,
    );
    if read_task_mountinfo(pid, tid, &mut inventory.issues).as_deref() != Some(&bytes) {
        add_issue(&mut inventory.issues, "task_mountinfo_changed");
    }
}

fn scan_descriptors(pid: u32, tid: u32, context: &ScanContext<'_>, inventory: &mut Inventory) {
    let fd_path = format!("{PROC}/{pid}/task/{tid}/fd");
    let Ok(fds) = fs::read_dir(fd_path) else {
        add_issue(&mut inventory.issues, "task_fd_list_unreadable");
        return;
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
        observe_task_descriptor(&fd.path(), context, inventory);
    }
}

fn observe_task_descriptor(
    path: &std::path::Path,
    context: &ScanContext<'_>,
    inventory: &mut Inventory,
) {
    let Ok(observation) = observe_descriptor(path) else {
        add_issue(&mut inventory.issues, "task_fd_observation_incomplete");
        return;
    };
    let namespace_reference = namespace_link(&observation.target).is_some_and(|inode| {
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
    let tmpfs_reference = observation.filesystem_magic == tmpfs_magic()
        && context.tmpfs_devices.contains(&linux_device(
            u64::from(observation.device[0]),
            u64::from(observation.device[1]),
        ));
    if namespace_reference || tmpfs_reference {
        inventory.owned_reference_count = inventory.owned_reference_count.saturating_add(1);
    }
}
