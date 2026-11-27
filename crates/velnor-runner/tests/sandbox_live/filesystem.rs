// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::support::{Sandbox, failed, succeeded, text};

const HIDDEN: [&str; 10] = [
    "/var/lib/velnor-host",
    "/var/lib/velnor/locks",
    "/var/lib/velnor/jit",
    "/opt/orbstack-guest/run",
    "/opt/orbstack-guest/data",
    "/mnt",
    "/mnt/mac",
    "/mnt/machines",
    "/run/user",
    "/Users",
];

#[test]
fn the_tools_store_is_read_only_and_root_owned_down_from_the_root() {
    let sandbox = Sandbox::new("tools-store");
    let owners = succeeded(&sandbox.shell(
        "for path in / /var /var/lib /var/lib/velnor /var/lib/velnor/tools /var/lib/velnor/tools/key; do stat -c '%u %a' $path; done",
    ));
    for line in owners.lines() {
        let (owner, mode) = line.split_once(' ').unwrap_or_default();
        assert_eq!(owner, "0", "{owners}");
        assert!(mode.ends_with("55") || mode.ends_with("00"), "{owners}");
    }
    let write = failed(&sandbox.shell("touch /var/lib/velnor/tools/x 2>&1"));
    assert!(write.contains("Read-only file system"), "{write}");
    assert!(
        succeeded(&sandbox.shell("find /var/lib/velnor/tools ! -uid 0")).is_empty(),
        "a file of the tools store is not root-owned"
    );
    for path in [
        "/usr/x",
        "/etc/x",
        "/x",
        "/srv/velnor/gen/x",
        "/var/lib/velnor/bin/x",
    ] {
        let write = failed(&sandbox.shell(&format!("touch {path} 2>&1")));
        assert!(write.contains("Read-only file system"), "{path}: {write}");
    }
}

#[test]
fn the_host_share_the_orbstack_runtime_and_the_mac_do_not_exist_in_the_job() {
    let sandbox = Sandbox::new("hidden-paths");
    for path in HIDDEN {
        let listing = failed(&sandbox.shell(&format!("ls {path} 2>&1")));
        assert!(
            listing.contains("No such file or directory"),
            "{path}: {listing}"
        );
    }
    let mounts = succeeded(&sandbox.shell("cat /proc/self/mountinfo"));
    for hidden in ["virtiofs", "/mnt/", "orbstack-guest/run", "velnor-host"] {
        assert!(!mounts.contains(hidden), "{hidden} is mounted:\n{mounts}");
    }
    let tools = failed(&sandbox.shell("which orb mac 2>&1"));
    assert!(!tools.contains("/bin/orb"), "{tools}");
}

#[test]
fn the_job_resolves_names_and_writes_only_its_own_directories() {
    let sandbox = Sandbox::new("writable");
    let resolved = succeeded(&sandbox.shell("getent hosts github.com"));
    assert!(resolved.contains("github.com"), "{resolved}");
    let stage = format!("/srv/velnor/gen/stage/{}/s", sandbox.reservation);
    let write = sandbox.shell(&format!(
        "touch \"$HOME/h\" /tmp/t \"$PWD/w\" {stage} && echo \"$HOME $PWD\""
    ));
    let both = succeeded(&write);
    assert!(both.contains("/home/live /srv/velnor/gen/work/"), "{both}");
    let work = sandbox.work_dir();
    assert!(!work.exists(), "the work dir survived the job");
}

#[test]
fn another_jobs_work_dir_is_invisible_and_this_ones_is_present() {
    let other = Path::new("/srv/velnor/gen/work/424242");
    assert!(std::fs::create_dir_all(other.join("secret")).is_ok());
    assert!(std::fs::write(other.join("secret/token"), "other job").is_ok());
    let sandbox = Sandbox::new("other-job");
    let own = sandbox.reservation.to_string();
    let listing = succeeded(&sandbox.shell("ls /srv/velnor/gen/work /srv/velnor/gen/stage"));
    assert_eq!(listing.matches(&own).count(), 2, "{listing}");
    assert!(!listing.contains("424242"), "{listing}");
    let read = failed(&sandbox.shell("cat /srv/velnor/gen/work/424242/secret/token 2>&1"));
    assert!(read.contains("No such file or directory"), "{read}");
    let _ = std::fs::remove_dir_all(other);
}

#[test]
fn one_file_past_16_gib_ends_the_write_with_sigxfsz() {
    let sandbox = Sandbox::new("file-size");
    let below = sandbox.shell("fallocate -l 1G \"$PWD/below\" && echo ok");
    assert!(succeeded(&below).contains("ok"), "{}", text(&below));
    let above = sandbox.shell("fallocate -l 17G \"$PWD/above\"");
    assert_eq!(above.status.code(), Some(153), "{}", text(&above));
}

fn killed_for_disk(sandbox: &Sandbox, script: &str) {
    let output = sandbox.shell(script);
    let log = text(&output);
    assert_eq!(output.status.code(), Some(125), "{log}");
    assert!(
        log.contains("velnor: job used ") && log.contains("GiB of disk; the limit is 1 GiB"),
        "{log}"
    );
    assert!(!sandbox.work_dir().exists());
    assert!(
        !Path::new(&format!(
            "/sys/fs/cgroup/velnor/job-{}",
            sandbox.reservation
        ))
        .exists(),
        "the cgroup of the killed job survived"
    );
}

#[test]
fn a_write_loop_is_killed_once_the_job_uses_more_than_its_limit() {
    let sandbox = Sandbox::new("disk-loop").with("--disk-limit-gib", "1");
    killed_for_disk(
        &sandbox,
        "i=0; while [ $i -lt 12 ]; do dd if=/dev/zero of=\"$PWD/f$i\" bs=1M count=256 2>/dev/null; i=$((i+1)); done",
    );
}

#[test]
fn an_unlinked_file_held_open_counts_against_the_limit() {
    let sandbox = Sandbox::new("disk-deleted").with("--disk-limit-gib", "1");
    killed_for_disk(
        &sandbox,
        "exec 3>\"$PWD/hidden\"; rm \"$PWD/hidden\"; dd if=/dev/zero bs=1M count=3000 >&3 2>/dev/null; sleep 30",
    );
}

#[test]
fn the_job_cannot_reach_the_mac_or_the_orbstack_tools() {
    let sandbox = Sandbox::new("orbstack");
    for command in ["orb list", "mac uname", "ls /opt/orbstack-guest/run"] {
        let output = failed(&sandbox.shell(&format!("{command} 2>&1")));
        assert!(
            output.contains("not found") || output.contains("No such file"),
            "{command}: {output}"
        );
    }
    let connect = failed(&sandbox.shell(
        "python3 -c 'import socket; socket.create_connection((\"host.orb.internal\", 22), 5)' 2>&1",
    ));
    assert!(
        connect.contains("refused")
            || connect.contains("not known")
            || connect.contains("unreachable"),
        "{connect}"
    );
}

#[test]
fn the_generations_and_the_stores_are_on_btrfs() {
    let output = std::process::Command::new("stat")
        .args(["-f", "-c", "%T", "/srv/velnor/gen", "/var/lib/velnor"])
        .output();
    assert_eq!(
        output
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
            .ok()
            .as_deref(),
        Some("btrfs\nbtrfs\n")
    );
}
