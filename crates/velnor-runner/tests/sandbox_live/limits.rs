// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::support::{
    BTRFS_IOCTL, Sandbox, cgroup, cleanup, joined, nothing_left, processes_running, run_dir,
    succeeded, text, unstarted, wait_until,
};

#[test]
fn one_file_past_16_gib_ends_the_write_with_sigxfsz() {
    let sandbox = Sandbox::new("file-size");
    let below = sandbox.shell("fallocate -l 1G \"$PWD/below\" && echo ok");
    assert!(succeeded(&below).contains("ok"), "{}", text(&below));
    let above = sandbox.shell("fallocate -l 17G \"$PWD/above\"");
    assert_eq!(above.status.code(), Some(153), "{}", text(&above));
}

struct SubvolumeCleaned<'a>(&'a Sandbox);

impl Drop for SubvolumeCleaned<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(self.0.work_dir().join("sneaky"));
        let _ = cleanup(self.0);
    }
}

fn breach_record(sandbox: &Sandbox) -> String {
    std::fs::read_to_string(run_dir(sandbox).join("breach.json")).unwrap_or_default()
}

fn killed_with(sandbox: &Sandbox, script: &[&str], message: &str, kind: &str) {
    let output = sandbox.run(script);
    let log = text(&output);
    assert_eq!(output.status.code(), Some(125), "{log}");
    assert!(log.contains(message), "{log}");
    assert!(
        !log.contains("survived"),
        "the script outlived the kill: {log}"
    );
    assert_eq!(
        breach_record(sandbox).trim(),
        format!("{{\"breach\":\"{kind}\"}}")
    );
    assert!(
        nothing_left(sandbox),
        "the killed job left files or its cgroup"
    );
    assert!(cleanup(sandbox).status.success());
    assert!(
        !run_dir(sandbox).exists(),
        "the breach record outlived the cleanup"
    );
}

fn killed_for_disk(sandbox: &Sandbox, script: &str) {
    killed_with(
        sandbox,
        &["/bin/sh", "-c", script],
        "GiB of disk; the limit is 1 GiB",
        "disk",
    );
}

#[test]
fn a_visible_write_of_1_gib_over_a_1_gib_limit_is_killed() {
    let sandbox = Sandbox::new("disk-visible").with("--disk-limit-gib", "1");
    killed_for_disk(
        &sandbox,
        "dd if=/dev/zero of=\"$PWD/big\" bs=1M count=1100 2>/dev/null; sleep 5; echo survived",
    );
}

#[test]
fn a_job_that_exits_before_the_first_walk_is_still_checked_against_its_limit() {
    let sandbox = Sandbox::new("disk-short").with("--disk-limit-gib", "1");
    killed_for_disk(&sandbox, "fallocate -l 2G \"$PWD/big\"");
}

#[test]
fn a_visible_tree_within_1000_levels_is_counted_and_killed_at_the_limit() {
    let sandbox = Sandbox::new("deep-900").with("--disk-limit-gib", "1");
    killed_with(
        &sandbox,
        &["/usr/bin/python3", "-c", DEEP, "900", "3"],
        "GiB of disk; the limit is 1 GiB",
        "disk",
    );
}

const DEEP: &str = "import os, sys, time\nfor _ in range(int(sys.argv[1])):\n    os.mkdir('d')\n    os.chdir('d')\nfd = os.open('payload', os.O_WRONLY | os.O_CREAT)\nos.posix_fallocate(fd, 0, int(sys.argv[2]) << 30)\ntime.sleep(5)\nprint('survived')\n";

const SHARED: &str = "import mmap, multiprocessing, os, time\nfd = os.open('shared', os.O_RDWR | os.O_CREAT)\nos.ftruncate(fd, 65536)\nmap = mmap.mmap(fd, 65536)\nos.unlink('shared')\nos.close(fd)\nlock = multiprocessing.Lock()\nlock.acquire()\nlock.release()\ntime.sleep(3.5)\nprint('survived')\n";

#[test]
fn a_deleted_shared_mmap_and_a_multiprocessing_lock_do_not_kill_the_job() {
    let sandbox = Sandbox::new("shared-mmap").with("--disk-limit-gib", "1");
    let output = sandbox.run(&["/usr/bin/python3", "-c", SHARED]);
    assert!(succeeded(&output).contains("survived"), "{}", text(&output));
}

const HIDDEN_BY_CLONE_FILES: &str = "import os, sys, threading, time\ndef hide():\n    os.unshare(os.CLONE_FILES)\n    fd = os.open('hidden', os.O_WRONLY | os.O_CREAT)\n    os.unlink('hidden')\n    os.posix_fallocate(fd, 0, int(sys.argv[1]) << 30)\n    time.sleep(30)\nthreading.Thread(target=hide, daemon=True).start()\ntime.sleep(30)\nprint('survived')\n";

const QGROUP_FREE: &str = "id=$(findmnt -T /srv/velnor/gen -no OPTIONS | tr ',' '\\n' | sed -n 's/^subvolid=//p'); d=$(ls -d /sys/fs/btrfs/*/qgroups/0_$id | head -n 1); echo $(cat $d/max_referenced) $(cat $d/referenced) $(cat $d/rsv_data)";

fn qgroup_free_bytes() -> u64 {
    let counters = succeeded(&unstarted(
        std::process::Command::new("sh")
            .args(["-c", QGROUP_FREE])
            .output(),
    ));
    let numbers: Vec<u64> = counters
        .split_whitespace()
        .filter_map(|field| field.parse().ok())
        .collect();
    assert_eq!(
        numbers.len(),
        3,
        "the qgroup counters are unreadable: {counters}"
    );
    numbers[0].saturating_sub(numbers[1] + numbers[2])
}

fn hidden_gib_and_floor() -> (String, String) {
    let free_gib = qgroup_free_bytes() >> 30;
    let hidden = free_gib.min(3);
    assert!(
        hidden >= 2,
        "the machine has {free_gib} GiB free; the hidden-data rows need at least 2"
    );
    (hidden.to_string(), (free_gib - hidden + 1).to_string())
}

fn qgroup_cap_gib() -> u64 {
    let limit = succeeded(&unstarted(
        std::process::Command::new("sh")
            .args(["-c", QGROUP_LIMIT])
            .output(),
    ));
    limit.trim().parse::<u64>().map_or(0, |bytes| bytes >> 30)
}

const QGROUP_LIMIT: &str = "id=$(findmnt -T /srv/velnor/gen -no OPTIONS | tr ',' '\\n' | sed -n 's/^subvolid=//p'); for d in /sys/fs/btrfs/*/qgroups/0_$id; do cat $d/max_referenced; done | head -n 1";

#[test]
fn data_a_clone_files_thread_hides_from_the_walk_is_stopped_by_the_floor_not_the_job_limit() {
    let (hidden, floor) = hidden_gib_and_floor();
    let sandbox = Sandbox::new("hidden-clone-files")
        .with("--disk-limit-gib", "1")
        .with("--machine-floor-gib", &floor);
    killed_with(
        &sandbox,
        &["/usr/bin/python3", "-c", HIDDEN_BY_CLONE_FILES, &hidden],
        "velnor: the machine's disk is nearly full",
        "machine_full",
    );
}

#[test]
fn a_tree_nested_past_1024_levels_is_not_counted_and_the_floor_stops_it() {
    let (hidden, floor) = hidden_gib_and_floor();
    let sandbox = Sandbox::new("deep-1500")
        .with("--disk-limit-gib", "1")
        .with("--machine-floor-gib", &floor);
    killed_with(
        &sandbox,
        &["/usr/bin/python3", "-c", DEEP, "1500", &hidden],
        "velnor: the machine's disk is nearly full",
        "machine_full",
    );
}

#[test]
fn a_tree_nested_1_000_000_levels_is_removed_in_under_a_minute_with_flat_descriptors() {
    let sandbox = Sandbox::new("deep-million");
    assert!(cleanup(&sandbox).status.success());
    let work = sandbox.work_dir();
    assert!(std::fs::create_dir_all(&work).is_ok());
    let built = unstarted(
        std::process::Command::new("/usr/bin/python3")
            .args(["-c", MILLION])
            .arg(&work)
            .output(),
    );
    assert!(built.status.success(), "{}", text(&built));
    let started = Instant::now();
    let removed = unstarted(
        std::process::Command::new("sh")
            .args([
                "-c",
                "ulimit -n 64; exec \"$0\" sandbox-cleanup --reservation \"$1\"",
            ])
            .arg(super::support::RUNNER)
            .arg(sandbox.reservation.to_string())
            .output(),
    );
    let took = started.elapsed();
    assert!(removed.status.success(), "{}", text(&removed));
    assert!(took < Duration::from_secs(60), "the removal took {took:?}");
    assert!(nothing_left(&sandbox));
}

const MILLION: &str = "import os, sys\nos.chdir(sys.argv[1])\nfor _ in range(1000000):\n    os.mkdir('d')\n    os.chdir('d')\n";

#[test]
fn a_nearly_full_machine_is_read_from_the_qgroup_not_from_statvfs() {
    let cap_gib = qgroup_cap_gib();
    assert!(
        cap_gib > 0,
        "the machine has no max_referenced on its qgroup"
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
fn a_buffered_fill_to_one_gib_below_the_free_space_is_killed_nearly_full_within_a_second() {
    let free_mib = qgroup_free_bytes() >> 20;
    assert!(free_mib > 2048, "the machine has {free_mib} MiB free");
    let sandbox = Sandbox::new("floor-fill")
        .with("--disk-limit-gib", "4096")
        .with("--machine-floor-gib", "1");
    let fill_mib = free_mib - 1024 - 256;
    let script = format!(
        "dd if=/dev/zero of=\"$PWD/fill\" bs=1M count={fill_mib} 2>/dev/null; echo ready; fallocate -l 512M \"$PWD/over\"; i=0; while [ $i -lt 100 ]; do echo tick; sleep 0.05; i=$((i+1)); done; echo survived"
    );
    let mut command = sandbox.command(&["/bin/sh", "-c", &script]);
    let mut child = command
        .stdout(Stdio::piped())
        .spawn()
        .expect("start the sandbox");
    let stdout = child.stdout.take().expect("stdout");
    let mut ready = None;
    let mut last_tick = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        match line.as_str() {
            "ready" => ready = Some(Instant::now()),
            "tick" => last_tick = Some(Instant::now()),
            _ => {}
        }
    }
    let output = child.wait_with_output().expect("wait");
    let ready = ready.expect("the buffered fill never finished");
    let alive = last_tick.map_or(Duration::ZERO, |tick| tick.duration_since(ready));
    assert_eq!(output.status.code(), Some(125));
    assert!(
        alive < Duration::from_secs(1),
        "the job lived {alive:?} past the floor"
    );
    assert_eq!(
        breach_record(&sandbox).trim(),
        "{\"breach\":\"machine_full\"}"
    );
    assert!(cleanup(&sandbox).status.success());
}

#[test]
fn a_subvolume_created_under_the_job_directories_kills_the_job() {
    let sandbox = Sandbox::new("foreign-device");
    let _cleaned = SubvolumeCleaned(&sandbox);
    let mut child = sandbox.spawn(&["/bin/sleep", "607"]);
    assert!(joined(&sandbox), "the job never started");
    let made = succeeded(&unstarted(
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
    assert_eq!(
        breach_record(&sandbox).trim(),
        "{\"breach\":\"foreign_device\"}"
    );
    let wedged = cleanup(&sandbox);
    assert_eq!(wedged.status.code(), Some(70), "{}", text(&wedged));
    assert!(std::fs::remove_dir(sandbox.work_dir().join("sneaky")).is_ok());
    assert!(cleanup(&sandbox).status.success());
    assert!(nothing_left(&sandbox));
}

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
