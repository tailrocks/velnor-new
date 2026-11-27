// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::support::{GID, Sandbox, UID, failed, succeeded, text};

fn status_field(sandbox: &Sandbox, field: &str) -> String {
    let script = format!("grep '^{field}:' /proc/self/status");
    succeeded(&sandbox.shell(&script)).trim().to_owned()
}

#[test]
fn the_job_runs_as_its_user_with_one_group_and_no_privilege() {
    let sandbox = Sandbox::new("identity");
    assert_eq!(
        succeeded(&sandbox.run(&["/usr/bin/id", "-G"])).trim(),
        GID.to_string()
    );
    assert_eq!(
        succeeded(&sandbox.run(&["/usr/bin/id", "-u"])).trim(),
        UID.to_string()
    );
    assert_eq!(status_field(&sandbox, "NoNewPrivs"), "NoNewPrivs:\t1");
    assert_eq!(
        status_field(&sandbox, "CapEff"),
        "CapEff:\t0000000000000000"
    );
    assert_eq!(status_field(&sandbox, "Seccomp"), "Seccomp:\t2");
    let sudo = failed(&sandbox.shell("sudo -n true 2>&1"));
    assert!(sudo.contains("no new privileges"), "{sudo}");
}

#[test]
fn the_job_is_pid_one_of_its_own_pid_namespace() {
    let sandbox = Sandbox::new("pid-namespace");
    assert_eq!(succeeded(&sandbox.shell("echo $$")).trim(), "1");
    let listing = succeeded(&sandbox.shell("ls /proc | grep -c '^[0-9]'"));
    assert!(
        listing.trim().parse::<u32>().is_ok_and(|count| count < 10),
        "the job sees the processes of the machine: {listing}"
    );
}

#[test]
fn the_job_cannot_create_user_namespaces_or_open_a_vsock_and_can_still_fork() {
    let sandbox = Sandbox::new("escapes");
    let unshare = failed(&sandbox.shell("unshare --user true 2>&1"));
    assert!(unshare.contains("unshare failed"), "{unshare}");
    let vsock = failed(&sandbox.shell(
        "python3 -c 'import socket; socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM)' 2>&1",
    ));
    assert!(vsock.contains("Address family not supported"), "{vsock}");
    let inet = sandbox
        .shell("python3 -c 'import socket; socket.socket(socket.AF_INET, socket.SOCK_STREAM)'");
    assert!(inet.status.success(), "{}", text(&inet));
    let forked = sandbox.shell(
        "python3 -c 'import os; pid = os.fork()\nos._exit(0) if pid == 0 else os.waitpid(pid, 0)'; (true); echo forked",
    );
    assert!(succeeded(&forked).contains("forked"), "{}", text(&forked));
}

#[test]
fn abstract_unix_sockets_outside_the_sandbox_are_unreachable() {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener};

    let name = format!("velnor-live-{}", std::process::id());
    let address = SocketAddr::from_abstract_name(name.as_bytes()).ok();
    let listener = address.and_then(|address| UnixListener::bind_addr(&address).ok());
    assert!(
        listener.is_some(),
        "the test could not bind an abstract socket"
    );
    let sandbox = Sandbox::new("abstract-sockets");
    let outside = failed(&sandbox.run(&[
        "/usr/bin/python3",
        "-c",
        &format!("import socket\ns = socket.socket(socket.AF_UNIX)\ns.connect('\\0{name}')\n"),
    ]));
    assert!(outside.contains("PermissionError"), "{outside}");
    let inside = sandbox.run(&[
        "/usr/bin/python3",
        "-c",
        "import socket\nserver = socket.socket(socket.AF_UNIX)\nserver.bind('\\0inside')\nserver.listen(1)\nclient = socket.socket(socket.AF_UNIX)\nclient.connect('\\0inside')\n",
    ]);
    assert!(inside.status.success(), "{}", text(&inside));
}
