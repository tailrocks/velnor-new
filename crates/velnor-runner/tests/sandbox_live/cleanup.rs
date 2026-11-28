// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};

use super::support::{
    BTRFS_IOCTL, Cleaned, Sandbox, cgroup, cleanup, joined, nothing_left, processes_running,
    succeeded, wait_until,
};

#[test]
fn a_daemon_that_leaves_its_session_dies_with_the_job_and_the_cgroup_is_gone() {
    let sandbox = Sandbox::new("daemon");
    let output = sandbox
        .shell("setsid sleep 601 >/dev/null 2>&1 </dev/null & sleep 0.2; cat /proc/self/cgroup");
    assert!(succeeded(&output).contains("/velnor/job-"));
    assert_eq!(
        processes_running("sleep", "601"),
        0,
        "the daemon survived the job"
    );
    assert!(nothing_left(&sandbox), "the job left its cgroup or dirs");
}

#[test]
fn a_second_signal_kills_the_whole_cgroup_and_removes_it() {
    let sandbox = Sandbox::new("signals");
    let mut child = sandbox.spawn(&["/bin/sh", "-c", "trap '' TERM; exec /bin/sleep 602"]);
    let pid = child
        .as_ref()
        .and_then(|child| i32::try_from(child.id()).ok())
        .and_then(Pid::from_raw);
    assert!(pid.is_some(), "the sandbox did not start");
    let started = joined(&sandbox) && wait_until(|| processes_running("/bin/sleep", "602") == 1);
    if let Some(pid) = pid {
        let _ = kill_process(pid, Signal::TERM);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            processes_running("/bin/sleep", "602"),
            1,
            "one SIGTERM ended a process that ignores it"
        );
        let _ = kill_process(pid, Signal::TERM);
    }
    let mut status = None;
    let ended = wait_until(|| {
        status = child
            .as_mut()
            .and_then(|child| child.try_wait().ok())
            .flatten();
        status.is_some()
    });
    if !ended {
        let _ = child.as_mut().map(std::process::Child::kill);
    }
    assert!(started, "the job never started");
    assert!(ended, "the second signal did not end the sandbox");
    assert!(status.is_some_and(|status| !status.success()));
    assert_eq!(processes_running("/bin/sleep", "602"), 0);
    assert!(nothing_left(&sandbox));
}

#[test]
fn killing_the_sandbox_parent_ends_the_job_and_the_cleanup_leaves_nothing() {
    let sandbox = Sandbox::new("parent-killed");
    let _cleaned = Cleaned(&sandbox);
    let mut child = sandbox.spawn(&["/bin/sleep", "603"]);
    assert!(
        joined(&sandbox) && wait_until(|| processes_running("/bin/sleep", "603") == 1),
        "the job never started"
    );
    let _ = child.as_mut().map(std::process::Child::kill);
    let _ = child.as_mut().map(std::process::Child::wait);
    assert!(
        wait_until(|| processes_running("/bin/sleep", "603") == 0),
        "the job survived the death of its sandbox parent"
    );
    assert!(
        wait_until(
            || std::fs::read_to_string(cgroup(&sandbox).join("cgroup.events"))
                .is_ok_and(|events| events.contains("populated 0"))
        ),
        "the cgroup of the dead sandbox is still populated"
    );
    let proof = cleanup(&sandbox);
    assert!(proof.status.success(), "{}", super::support::text(&proof));
    assert!(nothing_left(&sandbox));
}

#[test]
fn every_dead_sandbox_is_swept_with_its_cgroup_and_directories() {
    let sandboxes = [Sandbox::new("sweep-one"), Sandbox::new("sweep-two")];
    let mut children: Vec<_> = sandboxes
        .iter()
        .map(|sandbox| sandbox.spawn(&["/bin/sleep", "604"]))
        .collect();
    for sandbox in &sandboxes {
        assert!(joined(sandbox), "a job never started");
    }
    for child in children.iter_mut().flatten() {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(wait_until(|| processes_running("/bin/sleep", "604") == 0));
    let swept = super::support::unstarted(
        std::process::Command::new(super::support::RUNNER)
            .args(["sandbox-cleanup", "--all"])
            .output(),
    );
    assert!(swept.status.success(), "{}", super::support::text(&swept));
    for sandbox in &sandboxes {
        assert!(nothing_left(sandbox));
    }
    let leftovers = super::support::unstarted(
        std::process::Command::new("sh")
            .args(["-c", "ls /sys/class/net | grep -c '^vj'; ls /run/netns 2>/dev/null | grep -c '^velnor-job-'"])
            .output(),
    );
    assert_eq!(
        String::from_utf8_lossy(&leftovers.stdout)
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["0", "0"],
        "veth devices or job namespaces survived the sweep"
    );
}

#[test]
fn zombies_of_orphaned_processes_are_reaped_by_the_init_of_the_job() {
    let sandbox = Sandbox::new("reaper");
    let output = sandbox.shell(
        "(sh -c 'sleep 0.3' &) ; sleep 1.5; grep -l '^State:.Z' /proc/[0-9]*/status | wc -l",
    );
    assert_eq!(succeeded(&output).trim(), "0");
}

#[test]
fn a_subvolume_inside_a_job_directory_wedges_the_cleanup_with_a_typed_exit_and_it_converges_after()
{
    use std::os::unix::fs::MetadataExt;
    let sandbox = Sandbox::new("nested-subvolume");
    let _cleaned = Cleaned(&sandbox);
    assert!(cleanup(&sandbox).status.success());
    let work = sandbox.work_dir();
    assert!(std::fs::create_dir_all(&work).is_ok());
    let made = succeeded(&super::support::unstarted(
        std::process::Command::new("/usr/bin/python3")
            .args(["-c", BTRFS_IOCTL])
            .arg(&work)
            .args(["5000940e", "nested"])
            .output(),
    ));
    assert_eq!(made.trim(), "created");
    let nested = work.join("nested");
    let devices = (
        std::fs::metadata(&nested)
            .map(|metadata| metadata.dev())
            .ok(),
        std::fs::metadata(&work).map(|metadata| metadata.dev()).ok(),
    );
    assert_ne!(
        devices.0, devices.1,
        "the nested subvolume has its own device"
    );
    let wedged = cleanup(&sandbox);
    assert_eq!(
        wedged.status.code(),
        Some(70),
        "{}",
        super::support::text(&wedged)
    );
    assert!(
        nested.exists(),
        "the subvolume was removed behind the wedge"
    );
    assert!(std::fs::remove_dir(&nested).is_ok());
    let proof = cleanup(&sandbox);
    assert!(proof.status.success(), "{}", super::support::text(&proof));
    assert!(nothing_left(&sandbox));
}
