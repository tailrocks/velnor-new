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
    for field in ["CapEff", "CapPrm", "CapInh", "CapBnd", "CapAmb"] {
        assert_eq!(
            status_field(&sandbox, field),
            format!("{field}:\t0000000000000000")
        );
    }
    assert_eq!(status_field(&sandbox, "Seccomp"), "Seccomp:\t2");
    let sudo = failed(&sandbox.shell("sudo -n true 2>&1"));
    assert!(sudo.contains("no new privileges"), "{sudo}");
}

#[test]
fn the_job_runs_in_its_own_pid_namespace_under_a_root_init() {
    let sandbox = Sandbox::new("pid-namespace");
    assert_eq!(succeeded(&sandbox.shell("echo $$")).trim(), "2");
    let init = succeeded(&sandbox.shell("stat -c '%u' /proc/1"));
    assert_eq!(init.trim(), "0");
    let listing = succeeded(&sandbox.shell("ls /proc | grep -c '^[0-9]'"));
    assert!(
        listing.trim().parse::<u32>().is_ok_and(|count| count < 10),
        "the job sees the processes of the machine: {listing}"
    );
}

#[test]
fn the_job_cannot_create_user_namespaces_and_can_still_fork() {
    let sandbox = Sandbox::new("escapes");
    let unshare = failed(&sandbox.shell("unshare --user true 2>&1"));
    assert!(unshare.contains("unshare failed"), "{unshare}");
    let inet = sandbox
        .shell("python3 -c 'import socket; socket.socket(socket.AF_INET, socket.SOCK_STREAM)'");
    assert!(inet.status.success(), "{}", text(&inet));
    let forked = sandbox.shell(
        "python3 -c 'import os; pid = os.fork()\nos._exit(0) if pid == 0 else os.waitpid(pid, 0)'; (true); echo forked",
    );
    assert!(succeeded(&forked).contains("forked"), "{}", text(&forked));
}

#[test]
fn abstract_unix_sockets_of_the_job_netns_outside_the_sandbox_are_unreachable() {
    let name = format!("velnor-live-{}", std::process::id());
    let sandbox = Sandbox::new("abstract-sockets");
    let script = format!(
        "import socket, time\nresult = 'timeout'\nfor _ in range(100):\n    s = socket.socket(socket.AF_UNIX)\n    try:\n        s.connect('\\0{name}')\n        result = 'connected'\n        break\n    except PermissionError:\n        result = 'PermissionError'\n        break\n    except OSError:\n        time.sleep(0.1)\nprint(result)\n"
    );
    let started = sandbox.spawn(&["/usr/bin/python3", "-c", &script]);
    assert!(started.is_some(), "the sandbox did not start");
    let Some(child) = started else {
        return;
    };
    assert!(super::support::joined(&sandbox), "the job never started");
    let listener = std::process::Command::new("ip")
        .args([
            "netns",
            "exec",
            &format!("velnor-job-{}", sandbox.reservation),
            "python3",
            "-c",
            &format!(
                "import socket, time\ns = socket.socket(socket.AF_UNIX)\ns.bind('\\0{name}')\ns.listen()\ntime.sleep(8)\n"
            ),
        ])
        .spawn();
    let output = child.wait_with_output();
    let _ = listener.map(|mut listener| listener.kill());
    let outside = output.map_or_else(|_| String::new(), |output| succeeded(&output));
    assert_eq!(outside.trim(), "PermissionError", "{outside}");
    let inside = sandbox.run(&[
        "/usr/bin/python3",
        "-c",
        "import socket\nserver = socket.socket(socket.AF_UNIX)\nserver.bind('\\0inside')\nserver.listen(1)\nclient = socket.socket(socket.AF_UNIX)\nclient.connect('\\0inside')\n",
    ]);
    assert!(inside.status.success(), "{}", text(&inside));
}
