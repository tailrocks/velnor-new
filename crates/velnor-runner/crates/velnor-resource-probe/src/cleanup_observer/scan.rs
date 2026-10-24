use std::time::Instant;

use crate::{
    ProbeError,
    cleanup_observation::{CleanupObservationRequest, observe_cleanup as observe_cgroup_cleanup},
};

use super::{CleanupObserverRecord, CleanupObserverRequest, ProcessIdentity};
mod helpers;
mod inventory;
mod mounts;
mod output;
#[cfg(test)]
mod tests;
use helpers::{add_issue, process_start_ticks, read_bounded};
use mounts::read_mountinfo;
use output::{ScanEvidence, guest_root_matches, record};

const PROC: &str = "/proc";
const BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";

pub(super) fn observe(
    request: CleanupObserverRequest,
) -> Result<CleanupObserverRecord, ProbeError> {
    let started = Instant::now();
    let Some(boot_id) = read_boot_id() else {
        return Ok(record(request, incomplete_boot_evidence()));
    };
    if boot_id != request.binding.expected_guest_boot_id {
        return Err(ProbeError::Invalid("cleanup_boot_id_mismatch"));
    }
    let mut issues = Vec::new();
    let root_before = observe_root(
        &request,
        "initial_cgroup_or_root_check_incomplete",
        &mut issues,
    );
    let daemon_mounts = read_mountinfo(request.binding.daemon.pid, &mut issues);
    check_process(
        &request.binding.daemon,
        &mut issues,
        "daemon_identity_unreadable",
    );
    let mut inventory = inventory::scan(&request, started);
    issues.append(&mut inventory.issues);
    let boot_after = read_boot_id();
    if boot_after.as_deref() != Some(boot_id.as_str()) {
        add_issue(&mut issues, "guest_identity_changed");
    }
    let root_after = observe_root(
        &request,
        "final_cgroup_or_root_check_incomplete",
        &mut issues,
    );
    check_root_stability(
        root_before.as_ref(),
        root_after.as_ref(),
        &request,
        &mut issues,
    );
    check_process(
        &request.binding.daemon,
        &mut issues,
        "daemon_identity_changed",
    );
    check_daemon_mounts(&request, daemon_mounts.as_deref(), &mut issues);
    Ok(record(
        request,
        ScanEvidence {
            guest_boot_id: boot_after,
            docker_root: root_after,
            issues,
            task_count: inventory.task_count,
            descriptor_count: inventory.descriptor_count,
            namespace_count: inventory.namespace_count,
            owned_reference_count: inventory.owned_reference_count,
        },
    ))
}

fn read_boot_id() -> Option<String> {
    read_bounded(BOOT_ID, 128, "cleanup_boot_id")
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|value| value.trim_end_matches('\n').to_owned())
}

fn incomplete_boot_evidence() -> ScanEvidence {
    ScanEvidence {
        guest_boot_id: None,
        docker_root: None,
        issues: vec!["boot_identity_unreadable".to_owned()],
        task_count: 0,
        descriptor_count: 0,
        namespace_count: 0,
        owned_reference_count: 0,
    }
}

fn observe_root(
    request: &CleanupObserverRequest,
    issue: &'static str,
    issues: &mut Vec<String>,
) -> Option<crate::cleanup_observation::GuestDockerRootIdentity> {
    let observation = observe_cgroup_cleanup(CleanupObservationRequest {
        schema_version: 1,
        operation_id: request.operation_id.clone(),
        expected_guest_boot_id: request.binding.expected_guest_boot_id.clone(),
        cgroup_paths: request.binding.cgroup_paths.clone(),
    });
    let Ok(observation) = observation else {
        add_issue(issues, issue);
        return None;
    };
    if !guest_root_matches(&observation.docker_root, &request.binding.docker_root) {
        add_issue(issues, "docker_root_mismatch");
    }
    Some(observation.docker_root)
}

fn check_root_stability(
    before: Option<&crate::cleanup_observation::GuestDockerRootIdentity>,
    after: Option<&crate::cleanup_observation::GuestDockerRootIdentity>,
    request: &CleanupObserverRequest,
    issues: &mut Vec<String>,
) {
    if before != after
        || after.is_none_or(|root| !guest_root_matches(root, &request.binding.docker_root))
    {
        add_issue(issues, "docker_root_changed");
    }
}

fn check_process(identity: &ProcessIdentity, issues: &mut Vec<String>, failure: &'static str) {
    let path = format!("{PROC}/{}/stat", identity.pid);
    if !matches!(process_start_ticks(&path, identity.pid), Ok(value) if value == identity.start_ticks)
    {
        add_issue(issues, failure);
        return;
    }
    let namespace_path = format!("{PROC}/{}/ns/mnt", identity.pid);
    if !matches!(
        helpers::namespace_identity(&namespace_path),
        Ok(value) if value == (identity.namespace_device, identity.namespace_inode)
    ) {
        add_issue(issues, failure);
    }
}

fn check_daemon_mounts(
    request: &CleanupObserverRequest,
    before: Option<&[u8]>,
    issues: &mut Vec<String>,
) {
    if read_mountinfo(request.binding.daemon.pid, issues).as_deref() != before {
        add_issue(issues, "daemon_mountinfo_changed");
    }
}
