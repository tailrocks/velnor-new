// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support::{
    Sandbox, breach_record, cgroup, cleanup, joined, killed_with, nothing_left, processes_running,
    text, wait_until,
};

#[test]
fn a_job_the_kernel_kills_for_memory_is_a_breach_with_its_record() {
    let sandbox = Sandbox::new("oom").with("--max-jobs", "1024");
    killed_with(
        &sandbox,
        &[
            "/usr/bin/python3",
            "-c",
            "data = b'x' * (1 << 30)\nprint('survived', len(data))\n",
        ],
        "velnor: job was killed for using more memory than its limit",
        "memory",
    );
}

#[test]
fn a_job_that_kills_the_process_the_sandbox_waits_for_is_a_breach_with_its_record() {
    let sandbox = Sandbox::new("self-kill");
    killed_with(
        &sandbox,
        &["/bin/sh", "-c", "kill -9 $$"],
        "velnor: the job killed the process the sandbox waits for",
        "killed",
    );
}

#[test]
fn a_fork_bomb_is_contained_and_the_job_is_killed_cleanly() {
    let sandbox = Sandbox::new("fork-bomb");
    let output = sandbox.shell("while :; do sleep 605 & done");
    let log = text(&output);
    assert_eq!(output.status.code(), Some(125), "{log}");
    assert!(
        log.contains("velnor: job started more than 16384 processes"),
        "{log}"
    );
    assert_eq!(breach_record(&sandbox).trim(), "{\"breach\":\"processes\"}");
    assert!(nothing_left(&sandbox));
    assert!(wait_until(|| processes_running("sleep", "605") == 0));
    assert!(cleanup(&sandbox).status.success());
}

fn memory_max(max_jobs: &str) -> (u64, String, String) {
    let sandbox = Sandbox::new(&format!("cgroup-limits-{max_jobs}")).with("--max-jobs", max_jobs);
    let mut child = sandbox.spawn(&["/bin/sleep", "606"]);
    assert!(joined(&sandbox), "the job never started");
    let read = |file: &str| {
        std::fs::read_to_string(cgroup(&sandbox).join(file))
            .map_or_else(|_| String::new(), |text| text.trim().to_owned())
    };
    let memory: u64 = read("memory.max").parse().unwrap_or_default();
    let (pids, oom_group) = (read("pids.max"), read("memory.oom.group"));
    let _ = child.as_mut().map(std::process::Child::kill);
    let _ = child.as_mut().map(std::process::Child::wait);
    assert!(wait_until(|| processes_running("/bin/sleep", "606") == 0));
    assert!(cleanup(&sandbox).status.success());
    (memory, pids, oom_group)
}

#[test]
fn the_cgroup_of_a_job_limits_its_processes_and_takes_its_share_of_the_memory() {
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let total_kib: u64 = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or_default();
    for (max_jobs, divisor) in [("1", 1), ("2", 2)] {
        let (memory, pids, oom_group) = memory_max(max_jobs);
        assert_eq!(pids, "16384");
        assert_eq!(oom_group, "1");
        let expected = total_kib * 1024 / 100 * 90 / divisor;
        assert!(
            memory.abs_diff(expected) < total_kib * 1024 / 100,
            "max_jobs {max_jobs}: memory.max {memory} against {expected}"
        );
    }
}
