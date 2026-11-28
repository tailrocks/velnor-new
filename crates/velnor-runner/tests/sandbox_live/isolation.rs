// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader};
use std::process::Command;

use super::support::{BTRFS_IOCTL, Sandbox, failed, succeeded, text, unstarted};

const SYSCALL: &str = "import ctypes, os, sys\nlibc = ctypes.CDLL(None, use_errno=True)\nnumber = int(sys.argv[1])\nresult = libc.syscall(number, 0, 0, 0, 0, 0, 0)\nprint(result, ctypes.get_errno())\n";

fn syscall_outcome(output: &std::process::Output) -> (i64, i32) {
    let text = succeeded(output);
    let mut fields = text.split_whitespace();
    (
        fields.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        fields.next().and_then(|v| v.parse().ok()).unwrap_or(0),
    )
}

fn in_sandbox(sandbox: &Sandbox, number: i64) -> (i64, i32) {
    syscall_outcome(&sandbox.run(&["/usr/bin/python3", "-c", SYSCALL, &number.to_string()]))
}

fn outside(number: i64) -> (i64, i32) {
    syscall_outcome(&unstarted(
        Command::new("/usr/bin/python3")
            .args(["-c", SYSCALL, &number.to_string()])
            .output(),
    ))
}

#[test]
fn keyrings_io_uring_and_clone3_do_not_exist_for_the_job() {
    let sandbox = Sandbox::new("absent-calls");
    for (name, number) in [
        ("add_key", libc::SYS_add_key),
        ("request_key", libc::SYS_request_key),
        ("keyctl", libc::SYS_keyctl),
        ("io_uring_setup", libc::SYS_io_uring_setup),
        ("io_uring_enter", libc::SYS_io_uring_enter),
        ("io_uring_register", libc::SYS_io_uring_register),
        ("clone3", libc::SYS_clone3),
    ] {
        assert_eq!(in_sandbox(&sandbox, number), (-1, libc::ENOSYS), "{name}");
        assert_ne!(outside(number).1, libc::ENOSYS, "{name} outside");
    }
}

#[test]
fn extended_attributes_cannot_be_set_and_report_a_filesystem_without_them() {
    let sandbox = Sandbox::new("xattrs");
    let script = "import os\nopen('f', 'w').close()\ntry:\n    os.setxattr('f', 'user.k', b'v')\n    print('set')\nexcept OSError as error:\n    print(error.errno)\n";
    let inside = succeeded(&sandbox.run(&["/usr/bin/python3", "-c", script]));
    assert_eq!(inside.trim(), libc::ENOTSUP.to_string());
    let control = succeeded(&unstarted(
        Command::new("/usr/bin/python3")
            .args(["-c", script])
            .current_dir("/var/tmp")
            .output(),
    ));
    assert_eq!(control.trim(), "set");
    let _ = std::fs::remove_file("/var/tmp/f");
}

#[test]
fn a_host_vsock_cannot_be_opened_by_socket_or_by_io_uring() {
    let sandbox = Sandbox::new("vsock");
    let connect = "import socket\ns = socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM)\ns.settimeout(3)\ns.connect((socket.VMADDR_CID_HOST, 1234))\n";
    let refused = failed(&sandbox.run(&["/usr/bin/python3", "-c", connect]));
    assert!(
        refused.contains("Address family not supported"),
        "{refused}"
    );
    let control = unstarted(
        Command::new("/usr/bin/python3")
            .args(["-c", connect])
            .output(),
    );
    let reason = failed(&control);
    assert!(
        !reason.contains("Address family not supported") && reason.contains("OSError"),
        "the control connect must fail in the transport, not in the filter: {reason}"
    );
}

#[test]
fn two_concurrent_jobs_cannot_see_each_others_sysv_segments() {
    let first = Sandbox::new("ipc-first");
    let second = Sandbox::new("ipc-second");
    let Some(mut holder) =
        first.spawn(&["/bin/sh", "-c", "ipcmk -M 4096 && echo made && sleep 25"])
    else {
        panic!("the first sandbox did not start");
    };
    let stdout = holder
        .stdout
        .take()
        .expect("the first sandbox pipes its output");
    let made = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line == "made");
    let listing = succeeded(&second.shell("ipcs -m"));
    let _ = holder.kill();
    let _ = holder.wait();
    assert!(super::support::wait_until(|| super::support::cleanup(
        &first
    )
    .status
    .success()));
    assert!(made, "the first job never created its segment");
    let segments = listing
        .lines()
        .filter(|line| line.starts_with("0x"))
        .count();
    assert_eq!(
        segments, 0,
        "the second job sees a segment of the first:\n{listing}"
    );
}

fn machine_user() -> (u32, u32, String) {
    let listing = succeeded(&unstarted(
        Command::new("sh")
            .args([
                "-c",
                "for home in /home/*; do stat -c '%u %g %n' \"$home\"; done | head -n 1",
            ])
            .output(),
    ));
    let mut fields = listing.split_whitespace();
    let parsed = (|| {
        Some((
            fields.next()?.parse().ok()?,
            fields.next()?.parse().ok()?,
            fields.next()?.to_owned(),
        ))
    })();
    assert!(
        parsed.is_some(),
        "the machine has no ordinary user with a home directory"
    );
    parsed.unwrap_or_default()
}

#[test]
fn the_job_cannot_schedule_work_outside_its_cgroup_through_systemd_at_or_cron() {
    let (uid, gid, home) = machine_user();
    let sandbox = Sandbox::new("schedulers").running_as(uid, gid, &home);
    let control = succeeded(&unstarted(
        Command::new("setpriv")
            .args([
                "--reuid",
                &uid.to_string(),
                "--regid",
                &gid.to_string(),
                "--clear-groups",
                "sh",
                "-c",
                "echo '* * * * * true' | crontab - && crontab -l && crontab -r",
            ])
            .output(),
    ));
    assert!(control.contains("* * * * * true"), "{control}");
    for tool in ["systemd-run", "crontab"] {
        let present = succeeded(&sandbox.shell(&format!("command -v {tool}")));
        assert!(
            present.trim().starts_with('/'),
            "{tool} is not in the job root"
        );
    }
    let attempts = [
        "systemd-run --user --collect true",
        "echo '* * * * * true' | crontab -",
        "if command -v at >/dev/null; then echo true | at now; else exit 1; fi",
    ];
    for attempt in attempts {
        let output = sandbox.shell(&format!("{attempt} 2>&1"));
        assert!(!output.status.success(), "{attempt}: {}", text(&output));
    }
    let left = unstarted(
        Command::new("sh")
            .args(["-c", "ls /var/spool/cron/crontabs 2>/dev/null | wc -l"])
            .output(),
    );
    assert_eq!(succeeded(&left).trim(), "0", "a crontab was installed");
}

#[test]
fn btrfs_subvolume_snapshot_and_property_ioctls_answer_enotty_while_reflinks_work() {
    let sandbox = Sandbox::new("subvolumes");
    for request in ["5000940e", "50009418", "50009401", "50009417", "40089419"] {
        let inside =
            succeeded(&sandbox.run(&["/usr/bin/python3", "-c", BTRFS_IOCTL, ".", request, "sub"]));
        assert_eq!(inside.trim(), libc::ENOTTY.to_string(), "{request}");
    }
    let reflink = sandbox.shell(
        "head -c 1048576 /dev/zero > a && cp --reflink=always a b && cmp a b && echo cloned",
    );
    assert!(succeeded(&reflink).contains("cloned"), "{}", text(&reflink));
    let control = succeeded(&unstarted(
        Command::new("sh")
            .args([
                "-c",
                "d=$(mktemp -d /srv/velnor/gen/subvolume-control.XXXXXX) && python3 -c \"$0\" $d 5000940e sub; rmdir $d/sub; rmdir $d",
                BTRFS_IOCTL,
            ])
            .output(),
    ));
    assert_eq!(control.trim(), "created");
}
