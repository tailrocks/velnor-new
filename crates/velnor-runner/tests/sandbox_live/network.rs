// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::Command;

use super::support::{Sandbox, cleanup, succeeded, unstarted, wait_until};

const CONNECT: &str = "import socket, sys\ntry:\n    socket.create_connection((sys.argv[1], int(sys.argv[2])), 3).close()\n    print('connected')\nexcept OSError as error:\n    print('refused', error.errno)\n";

fn machine(script: &str) -> String {
    succeeded(&unstarted(Command::new("sh").args(["-c", script]).output()))
        .trim()
        .to_owned()
}

fn connects_from(sandbox: &Sandbox, address: &str, port: u16) -> bool {
    let output = sandbox.run(&[
        "/usr/bin/python3",
        "-c",
        CONNECT,
        address,
        &port.to_string(),
    ]);
    succeeded(&output).trim() == "connected"
}

#[test]
fn names_resolve_and_github_and_crates_io_answer_from_the_job() {
    let sandbox = Sandbox::new("net-reachable");
    let resolved = succeeded(&sandbox.shell("getent hosts github.com"));
    assert!(resolved.contains("github.com"), "{resolved}");
    for url in ["https://github.com", "https://index.crates.io/config.json"] {
        let status = sandbox.shell(&format!(
            "curl -sS -o /dev/null -m 20 -w '%{{http_code}}' {url}"
        ));
        let code = succeeded(&status);
        assert!(
            code.starts_with('2') || code.starts_with('3'),
            "{url}: {code}"
        );
    }
}

#[test]
fn the_machine_itself_is_refused_while_the_machine_reaches_its_own_listener() {
    let listener = TcpListener::bind("0.0.0.0:0").ok();
    assert!(listener.is_some(), "the test could not listen");
    let port = listener
        .as_ref()
        .and_then(|listener| listener.local_addr().ok())
        .map_or(0, |address| address.port());
    let address =
        machine("ip -4 -o addr show dev eth0 | awk '{print $4}' | cut -d/ -f1 | head -n 1");
    let local = unstarted(
        Command::new("python3")
            .args(["-c", CONNECT, &address, &port.to_string()])
            .output(),
    );
    assert_eq!(
        succeeded(&local).trim(),
        "connected",
        "the control must reach the listener"
    );
    let sandbox = Sandbox::new("net-machine");
    assert!(
        !connects_from(&sandbox, &address, port),
        "the job reached the machine at {address}"
    );
}

#[test]
fn one_job_cannot_reach_the_listener_of_another_while_the_listener_answers_itself() {
    let first = Sandbox::new("net-first");
    let second = Sandbox::new("net-second");
    let started = first.spawn(&[
        "/usr/bin/python3",
        "-c",
        "import socket, time\ns = socket.socket()\ns.bind(('0.0.0.0', 8123))\ns.listen()\nprint('up', flush=True)\ntime.sleep(25)\n",
    ]);
    assert!(started.is_some(), "the first sandbox did not start");
    let Some(mut holder) = started else {
        return;
    };
    let up = holder.stdout.take().is_some_and(|stdout| {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .any(|line| line == "up")
    });
    assert!(up, "the first job never listened");
    let address = machine(&format!(
        "ip -n velnor-job-{} -4 -o addr show dev eth0 | awk '{{print $4}}' | cut -d/ -f1",
        first.reservation
    ));
    let control = unstarted(
        Command::new("ip")
            .args([
                "netns",
                "exec",
                &format!("velnor-job-{}", first.reservation),
                "python3",
                "-c",
                CONNECT,
                &address,
                "8123",
            ])
            .output(),
    );
    let listening = succeeded(&control).trim() == "connected";
    let isolated = !connects_from(&second, &address, 8123);
    let _ = holder.kill();
    let _ = holder.wait();
    assert!(wait_until(|| cleanup(&first).status.success()));
    assert!(
        listening,
        "the listener of the first job does not answer at {address}"
    );
    assert!(
        isolated,
        "a job reached the listener of another job at {address}"
    );
}

#[test]
fn the_gateway_the_lan_and_the_mac_over_ipv6_are_refused() {
    let sandbox = Sandbox::new("net-lan");
    let gateway = machine("ip -4 route show default | awk '{print $3}' | head -n 1");
    for port in [22_u16, 80, 443, 445, 5000, 7000, 8080] {
        assert!(!connects_from(&sandbox, &gateway, port), "{gateway}:{port}");
    }
    let v6 = succeeded(&sandbox.run(&[
        "/usr/bin/python3",
        "-c",
        "import socket\ntry:\n    for family, kind, proto, name, address in socket.getaddrinfo('host.orb.internal', 22, socket.AF_INET6, socket.SOCK_STREAM):\n        socket.create_connection(address[:2], 3).close()\n        print('connected')\n        break\n    else:\n        print('no address')\nexcept OSError as error:\n    print('refused', error.errno)\n",
    ]));
    assert!(
        v6.starts_with("refused") || v6.starts_with("no address"),
        "{v6}"
    );
    let v4 =
        succeeded(&sandbox.run(&["/usr/bin/python3", "-c", CONNECT, "host.orb.internal", "22"]));
    assert!(v4.trim().starts_with("refused"), "{v4}");
}

#[test]
fn the_job_ruleset_is_loaded_in_the_machine_when_a_job_starts() {
    let sandbox = Sandbox::new("net-ruleset");
    let _ = unstarted(
        Command::new("nft")
            .args(["delete", "table", "inet", "velnor_job"])
            .output(),
    );
    succeeded(&sandbox.shell("true"));
    let listing = machine("nft list table inet velnor_job");
    for rule in [
        "iifname \"vj*\" reject",
        "masquerade",
        "meta nfproto ipv6 drop",
    ] {
        assert!(listing.contains(rule), "{rule}:\n{listing}");
    }
}

#[test]
fn a_job_has_its_own_network_namespace_and_leaves_none_behind() {
    let sandbox = Sandbox::new("net-own");
    let inside = succeeded(
        &sandbox.shell("readlink /proc/self/ns/net; ls /sys/class/net 2>/dev/null | tr '\\n' ' '"),
    );
    let own = inside.lines().next().unwrap_or_default().to_owned();
    let outside = machine("readlink /proc/self/ns/net");
    assert_ne!(own, outside, "{inside}");
    assert!(
        !std::path::Path::new(&format!("/run/netns/velnor-job-{}", sandbox.reservation)).exists(),
        "the netns of the finished job is still there"
    );
}
