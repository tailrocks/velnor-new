// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support::{
    BTRFS_IOCTL, Sandbox, cgroup, cleanup, joined, nothing_left, processes_running, succeeded,
    text, wait_until,
};

#[test]
fn one_file_past_16_gib_ends_the_write_with_sigxfsz() {
    let sandbox = Sandbox::new("file-size");
    let below = sandbox.shell("fallocate -l 1G \"$PWD/below\" && echo ok");
    assert!(succeeded(&below).contains("ok"), "{}", text(&below));
    let above = sandbox.shell("fallocate -l 17G \"$PWD/above\"");
    assert_eq!(above.status.code(), Some(153), "{}", text(&above));
}

fn killed_with(sandbox: &Sandbox, script: &[&str], message: &str) {
    let output = sandbox.run(script);
    let log = text(&output);
    assert_eq!(output.status.code(), Some(125), "{log}");
    assert!(log.contains(message), "{log}");
    assert!(
        nothing_left(sandbox),
        "the killed job left files or its cgroup"
    );
}

fn killed_for_disk(sandbox: &Sandbox, script: &str) {
    killed_with(
        sandbox,
        &["/bin/sh", "-c", script],
        "GiB of disk; the limit is 1 GiB",
    );
}

#[test]
fn a_write_loop_is_killed_once_the_job_uses_more_than_its_limit() {
    let sandbox = Sandbox::new("disk-loop").with("--disk-limit-gib", "1");
    killed_for_disk(
        &sandbox,
        "i=0; while [ $i -lt 20 ]; do dd if=/dev/zero of=\"$PWD/f$i\" bs=1M count=256 2>/dev/null; sleep 0.2; i=$((i+1)); done",
    );
}

#[test]
fn an_unlinked_file_held_open_counts_against_the_limit() {
    let sandbox = Sandbox::new("disk-deleted").with("--disk-limit-gib", "1");
    killed_for_disk(
        &sandbox,
        "exec 3>\"$PWD/hidden\"; rm \"$PWD/hidden\"; dd if=/dev/zero bs=1M count=1500 >&3 2>/dev/null; sleep 30",
    );
}

#[test]
fn an_unlinked_file_held_by_several_descriptors_counts_once() {
    let sandbox = Sandbox::new("disk-duplicates").with("--disk-limit-gib", "1");
    let output = sandbox.shell(
        "exec 3>\"$PWD/hidden\"; rm \"$PWD/hidden\"; dd if=/dev/zero bs=1M count=600 >&3 2>/dev/null; exec 4>&3 5>&3 6>&3; sleep 3; echo survived",
    );
    assert!(succeeded(&output).contains("survived"), "{}", text(&output));
}

#[test]
fn a_mapped_file_whose_descriptors_are_closed_counts_against_the_limit() {
    let sandbox = Sandbox::new("disk-mapped").with("--disk-limit-gib", "1");
    killed_with(
        &sandbox,
        &[
            "/usr/bin/python3",
            "-c",
            "import ctypes, os, time\nlibc = ctypes.CDLL(None, use_errno=True)\nlibc.mmap.restype = ctypes.c_void_p\nlibc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_long]\nfd = os.open('mapped', os.O_RDWR | os.O_CREAT)\nos.posix_fallocate(fd, 0, 3 << 29)\naddress = libc.mmap(None, 3 << 29, 3, 1, fd, 0)\nassert address not in (None, 2**64 - 1)\nos.unlink('mapped')\nos.close(fd)\ntime.sleep(30)\n",
        ],
        "velnor: job used",
    );
}

const DEEP: &str = "import os, sys, time\nfor _ in range(int(sys.argv[1])):\n    os.mkdir('d')\n    os.chdir('d')\nfd = os.open('payload', os.O_WRONLY | os.O_CREAT)\nos.posix_fallocate(fd, 0, 3 << 29)\ntime.sleep(30)\n";

#[test]
fn a_tree_nested_3000_levels_deep_is_still_counted_and_killed_at_the_limit() {
    let sandbox = Sandbox::new("deep-3000").with("--disk-limit-gib", "1");
    killed_with(
        &sandbox,
        &["/usr/bin/python3", "-c", DEEP, "3000"],
        "velnor: job used",
    );
}

#[test]
fn a_tree_nested_200000_levels_deep_is_killed_and_removed_without_a_trace() {
    let sandbox = Sandbox::new("deep-200000").with("--disk-limit-gib", "1");
    let output = sandbox.run(&["/usr/bin/python3", "-c", DEEP, "200000"]);
    let log = text(&output);
    assert_eq!(output.status.code(), Some(125), "{log}");
    assert!(log.contains("velnor: job "), "{log}");
    assert!(
        nothing_left(&sandbox),
        "the deep tree or the cgroup is still there"
    );
}

const QGROUP_LIMIT_GIB: &str = "id=$(findmnt -T /srv/velnor/gen -no OPTIONS | tr ',' '\\n' | sed -n 's/^subvolid=//p'); for d in /sys/fs/btrfs/*/qgroups/0_$id; do cat $d/max_referenced; done | head -n 1";

#[test]
fn a_nearly_full_machine_is_read_from_the_qgroup_not_from_statvfs() {
    let limit = succeeded(&super::support::unstarted(
        std::process::Command::new("sh")
            .args(["-c", QGROUP_LIMIT_GIB])
            .output(),
    ));
    let cap_gib = limit.trim().parse::<u64>().map_or(0, |bytes| bytes >> 30);
    assert!(
        cap_gib > 0,
        "the machine has no max_referenced on its qgroup: {limit}"
    );
    let sandbox = Sandbox::new("floor").with("--machine-floor-gib", &cap_gib.to_string());
    let output = sandbox.shell("sleep 30");
    let log = text(&output);
    assert_eq!(output.status.code(), Some(125), "{log}");
    assert!(
        log.contains("velnor: the machine's disk is nearly full"),
        "{log}"
    );
    assert!(nothing_left(&sandbox));
    let roomy = Sandbox::new("floor-roomy").with("--machine-floor-gib", "1");
    let ran = roomy.shell("echo fine");
    assert!(succeeded(&ran).contains("fine"), "{}", text(&ran));
}

#[test]
fn a_subvolume_created_under_the_job_directories_kills_the_job() {
    let sandbox = Sandbox::new("foreign-device");
    let mut child = sandbox.spawn(&["/bin/sleep", "607"]);
    assert!(joined(&sandbox), "the job never started");
    let made = succeeded(&super::support::unstarted(
        std::process::Command::new("/usr/bin/python3")
            .args(["-c", BTRFS_IOCTL])
            .arg(sandbox.work_dir())
            .args(["5000940e", "sneaky"])
            .output(),
    ));
    assert_eq!(made.trim(), "created");
    let mut status = None;
    let ended = wait_until(|| {
        status = child
            .as_mut()
            .and_then(|child| child.try_wait().ok())
            .flatten();
        status.is_some()
    });
    let log = child
        .take()
        .and_then(|child| child.wait_with_output().ok())
        .map_or_else(String::new, |output| text(&output));
    assert!(
        ended,
        "the job outlived a foreign device under its directory"
    );
    assert!(
        status.is_some_and(|status| status.code() == Some(125)),
        "{log}"
    );
    assert!(
        log.contains("velnor: job created a mount or a subvolume"),
        "{log}"
    );
    assert!(nothing_left(&sandbox));
}

#[test]
fn a_fork_bomb_is_contained_and_the_job_is_killed_cleanly() {
    let sandbox = Sandbox::new("fork-bomb");
    let output = sandbox.shell("while :; do sleep 605 & done");
    let log = text(&output);
    assert_eq!(output.status.code(), Some(125), "{log}");
    assert!(
        log.contains("velnor: job started more than 16384 processes")
            || log.contains("velnor: job has too many files to account for its disk use"),
        "{log}"
    );
    assert!(nothing_left(&sandbox));
    assert!(wait_until(|| processes_running("sleep", "605") == 0));
}

#[test]
fn the_cgroup_of_a_job_limits_its_processes_and_its_memory() {
    let sandbox = Sandbox::new("cgroup-limits");
    let mut child = sandbox.spawn(&["/bin/sleep", "606"]);
    assert!(joined(&sandbox), "the job never started");
    let read = |file: &str| {
        std::fs::read_to_string(cgroup(&sandbox).join(file))
            .map_or_else(|_| String::new(), |text| text.trim().to_owned())
    };
    let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let total_kib: u64 = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or_default();
    let pids = read("pids.max");
    let memory: u64 = read("memory.max").parse().unwrap_or_default();
    let oom_group = read("memory.oom.group");
    let _ = child.as_mut().map(std::process::Child::kill);
    let _ = child.as_mut().map(std::process::Child::wait);
    assert!(wait_until(|| processes_running("/bin/sleep", "606") == 0));
    assert!(cleanup(&sandbox).status.success());
    assert_eq!(pids, "16384");
    assert_eq!(oom_group, "1");
    let expected = total_kib * 1024 / 100 * 90;
    assert!(
        memory.abs_diff(expected) < total_kib * 1024 / 100,
        "memory.max {memory} against 90% of the machine {expected}"
    );
}
