// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

use super::support::{Sandbox, succeeded};

fn cgroup(sandbox: &Sandbox) -> PathBuf {
    PathBuf::from(format!("/sys/fs/cgroup/velnor/job-{}", sandbox.reservation))
}

fn processes_running(program: &str, argument: &str) -> usize {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            std::fs::read(entry.path().join("cmdline")).is_ok_and(|bytes| {
                let mut parts = bytes.split(|byte| *byte == 0);
                parts.next() == Some(program.as_bytes())
                    && parts.next() == Some(argument.as_bytes())
            })
        })
        .count()
}

fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

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
    assert!(
        !cgroup(&sandbox).exists(),
        "the job cgroup survived the job"
    );
}

#[test]
fn a_second_signal_kills_the_whole_cgroup_and_removes_it() {
    let sandbox = Sandbox::new("signals");
    let mut child = sandbox.command(&["/bin/sleep", "602"]).spawn().ok();
    let pid = child
        .as_ref()
        .and_then(|child| i32::try_from(child.id()).ok())
        .and_then(Pid::from_raw);
    assert!(pid.is_some(), "the sandbox did not start");
    let joined = wait_until(|| {
        std::fs::read_to_string(cgroup(&sandbox).join("cgroup.procs"))
            .is_ok_and(|procs| !procs.trim().is_empty())
    });
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
    assert!(joined, "the job never joined its cgroup");
    assert!(ended, "the second signal did not end the sandbox");
    assert!(status.is_some_and(|status| !status.success()));
    assert_eq!(processes_running("/bin/sleep", "602"), 0);
    assert!(!cgroup(&sandbox).exists());
}
