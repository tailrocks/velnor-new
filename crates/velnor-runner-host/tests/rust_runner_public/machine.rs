// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde_json::json;
use velnor_runner_host::{GuestUser, HostError, RustRunnerFailure};

use super::support::Fixture;

type Check = Result<(), Box<dyn Error>>;
type Mutation = fn(&mut serde_json::Value);

const SHARE: &str = "/var/lib/velnor-host";
const CAP_BYTES: u64 = 172 << 30;
const OPEN: [&str; 10] = [
    "var/lib/velnor",
    "var/lib/velnor/bin",
    "var/lib/velnor/tools",
    "var/lib/velnor/crates",
    "var/lib/velnor/cache",
    "var/lib/velnor/git",
    "srv/velnor",
    "srv/velnor/gen",
    "srv/velnor/gen/work",
    "srv/velnor/gen/stage",
];
const CLOSED: [&str; 4] = [
    "var/lib/velnor/locks",
    "var/lib/velnor/aside",
    "var/lib/velnor/incoming",
    "var/lib/velnor/live",
];

const fn refused(failure: RustRunnerFailure) -> HostError {
    HostError::RustRunner(failure)
}

fn guest(fixture: &Fixture, path: &str) -> PathBuf {
    fixture.root.join("guest").join(path)
}

fn mode(path: &PathBuf) -> Result<u32, Box<dyn Error>> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o7777)
}

fn commands(fixture: &Fixture, word: &str) -> Result<usize, Box<dyn Error>> {
    Ok(fixture
        .log()?
        .iter()
        .filter(|line| line.contains(&format!(" {word} ")))
        .count())
}

fn ensure_refused(fixture: &Fixture, failure: RustRunnerFailure) {
    assert_eq!(
        fixture.launcher().ensure_machine().err(),
        Some(refused(failure))
    );
    assert!(
        !guest(fixture, "var/lib/velnor/bin/velnor-runner").exists(),
        "a refused machine still got the runner"
    );
}

#[test]
fn rust_runner_creates_the_isolated_machine_with_the_disk_cap_and_the_share() -> Check {
    let fixture = Fixture::new("[]")?;
    fixture.launcher().ensure_machine()?;
    let log = fixture.log()?;
    assert_eq!(log[0], "list -f json");
    let create = format!(
        "create --isolated --isolate-network --disk 172G --mount {}:{SHARE} -a arm64 ubuntu velnor",
        fixture.store.display()
    );
    assert!(log.contains(&create), "{log:?}");
    fixture.launcher().ensure_machine()?;
    let creates = fixture
        .log()?
        .iter()
        .filter(|line| line.starts_with("create"))
        .count();
    assert_eq!(creates, 1);
    for name in ["bin", "jit", "trusted"] {
        assert!(fixture.store.join(name).is_dir(), "{name}");
    }
    let small = Fixture::new("[]")?;
    small.configure(&format!("FAKE_GUEST_CAP_BYTES={}\n", 8u64 << 30))?;
    small.launcher().with_machine_disk(8, 64).ensure_machine()?;
    assert!(
        small
            .log()?
            .iter()
            .any(|line| line.starts_with("create --isolated --isolate-network --disk 8G --mount ")),
        "{:?}",
        small.log()?
    );
    Ok(())
}

#[test]
fn rust_runner_keeps_a_machine_of_the_required_shape() -> Check {
    let fixture = Fixture::ready()?;
    fixture.launcher().ensure_machine()?;
    assert!(
        fixture
            .log()?
            .iter()
            .all(|line| !line.starts_with("create"))
    );
    for image in [
        json!({"distro": "debian", "arch": "arm64"}),
        json!({"distro": "ubuntu", "arch": "amd64"}),
    ] {
        let listed = Fixture::ready()?;
        let text = std::fs::read_to_string(listed.root.join("list.json"))?;
        let mut machines: serde_json::Value = serde_json::from_str(&text)?;
        machines[0]["image"] = image;
        std::fs::write(listed.root.join("list.json"), machines.to_string())?;
        assert_eq!(
            listed.launcher().ensure_machine().err(),
            Some(refused(RustRunnerFailure::MachineIncompatible))
        );
        assert!(listed.log()?.iter().all(|line| !line.contains(" -u root ")));
    }
    Ok(())
}

#[test]
fn rust_runner_refuses_a_machine_that_is_not_confined() -> Check {
    let breaks: [(&str, Mutation); 9] = [
        ("not isolated", |c| {
            c["isolated"] = json!(false);
        }),
        ("network not isolated", |c| {
            c["isolate_network"] = json!(false);
        }),
        ("ssh agent forwarded", |c| {
            c["forward_ssh_agent"] = json!(true);
        }),
        ("another cap", |c| {
            c["disk_limit_bytes"] = json!(8u64 << 30);
        }),
        ("no cap", |c| {
            if let Some(object) = c.as_object_mut() {
                object.remove("disk_limit_bytes");
            }
        }),
        ("no mount", |c| {
            c["mounts"] = json!([]);
        }),
        ("another mount source", |c| {
            c["mounts"][0]["source"] = json!("/opt/other-share");
        }),
        ("another mount target", |c| {
            c["mounts"][0]["destination"] = json!("/mnt/mac");
        }),
        ("an extra mount", |c| {
            if let Some(mounts) = c["mounts"].as_array_mut() {
                mounts.push(json!({"source": "/opt/home", "destination": "/mnt/home"}));
            }
        }),
    ];
    for (name, mutate) in breaks {
        let fixture = Fixture::shaped(mutate)?;
        assert_eq!(
            fixture.launcher().ensure_machine().err(),
            Some(refused(RustRunnerFailure::MachineShape)),
            "{name}"
        );
        let log = fixture.log()?;
        assert!(
            log.iter()
                .all(|line| !line.starts_with("create") && !line.contains(" -u root ")),
            "{name}: {log:?}"
        );
    }
    let today = Fixture::new(
        r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"},"config":{"isolated":false,"forward_ssh_agent":true,"isolate_network":false}}]"#,
    )?;
    let error = today.launcher().ensure_machine().err().ok_or("accepted")?;
    assert_eq!(error, refused(RustRunnerFailure::MachineShape));
    assert!(error.to_string().contains("orb delete velnor"));
    let resized = Fixture::ready()?;
    assert_eq!(
        resized
            .launcher()
            .with_machine_disk(8, 64)
            .ensure_machine()
            .err(),
        Some(refused(RustRunnerFailure::MachineShape))
    );
    Ok(())
}

#[test]
fn rust_runner_provisions_the_vm_local_store_layout() -> Check {
    let fixture = Fixture::ready()?;
    std::fs::write(fixture.root.join("host-runner"), "runner-build")?;
    std::fs::write(fixture.store.join("bin/velnor-runner"), "runner-build")?;
    fixture.launcher().ensure_machine()?;
    fixture.launcher().ensure_machine()?;
    for path in OPEN {
        assert!(guest(&fixture, path).is_dir(), "{path}");
        assert_eq!(mode(&guest(&fixture, path))?, 0o755, "{path}");
    }
    for path in CLOSED {
        assert_eq!(mode(&guest(&fixture, path))?, 0o700, "{path}");
    }
    let copy = guest(&fixture, "var/lib/velnor/bin/velnor-runner");
    assert_eq!(std::fs::read_to_string(&copy)?, "runner-build");
    assert_eq!(mode(&copy)?, 0o755);
    assert_eq!(commands(&fixture, "ln")?, 0);
    Ok(())
}

#[test]
fn rust_runner_refuses_an_ancestor_that_the_sandbox_user_could_own() -> Check {
    for owned in ["/var/lib", "/srv"] {
        let fixture = Fixture::ready()?;
        fixture.configure(&format!("FAKE_GUEST_OWNERS={owned}=1000\n"))?;
        ensure_refused(&fixture, RustRunnerFailure::MachineLayout);
    }
    let writable = Fixture::ready()?;
    writable.configure("FAKE_GUEST_MODES=/var=777\n")?;
    ensure_refused(&writable, RustRunnerFailure::MachineLayout);
    Ok(())
}

#[test]
fn rust_runner_refuses_stores_that_are_not_on_btrfs() -> Check {
    let fixture = Fixture::ready()?;
    fixture.configure("FAKE_GUEST_FS=xfs\n")?;
    ensure_refused(&fixture, RustRunnerFailure::MachineFilesystem);
    Ok(())
}

#[test]
fn rust_runner_refuses_a_machine_without_protected_links() -> Check {
    for values in ["1,0", "0,1", "0,0"] {
        let fixture = Fixture::ready()?;
        fixture.configure(&format!("FAKE_GUEST_SYSCTL={values}\n"))?;
        ensure_refused(&fixture, RustRunnerFailure::MachineShape);
    }
    Ok(())
}

#[test]
fn rust_runner_takes_the_runner_digest_from_the_host_not_from_the_share() -> Check {
    let fixture = Fixture::ready()?;
    let host = fixture.root.join("host-runner");
    let shared = fixture.store.join("bin/velnor-runner");
    let installed = guest(&fixture, "var/lib/velnor/bin/velnor-runner");
    std::fs::write(&host, "host-build")?;
    std::fs::write(&shared, "tampered")?;
    assert_eq!(
        fixture.launcher().ensure_machine().err(),
        Some(refused(RustRunnerFailure::BinaryCopy))
    );
    assert!(!installed.exists());
    std::fs::write(&shared, "host-build")?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(std::fs::read_to_string(&installed)?, "host-build");
    std::fs::write(&host, "next-build")?;
    std::fs::write(&shared, "next-build")?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(std::fs::read_to_string(&installed)?, "next-build");
    std::fs::write(&shared, "swapped-in-the-vm")?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(std::fs::read_to_string(&installed)?, "next-build");
    Ok(())
}

#[test]
fn rust_runner_probes_the_disk_cap_once_per_orbstack_version() -> Check {
    let fixture = Fixture::ready()?;
    let record = guest(&fixture, "var/lib/velnor/cap-probe");
    fixture.launcher().ensure_machine()?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(commands(&fixture, "fallocate")?, 1);
    assert_eq!(
        std::fs::read_to_string(&record)?.trim(),
        format!("orbstack=2.2.5-2020500 cap={CAP_BYTES}")
    );
    fixture.configure("FAKE_ORB_VERSION=2.3.0 (2030000)\n")?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(commands(&fixture, "fallocate")?, 2);
    assert_eq!(
        std::fs::read_to_string(&record)?.trim(),
        format!("orbstack=2.3.0-2030000 cap={CAP_BYTES}")
    );
    Ok(())
}

#[test]
fn rust_runner_refuses_a_machine_that_does_not_enforce_its_disk_cap() -> Check {
    let fixture = Fixture::ready()?;
    fixture.configure("FAKE_GUEST_NO_CAP=1\n")?;
    ensure_refused(&fixture, RustRunnerFailure::CapProbe);
    assert!(!guest(&fixture, "var/lib/velnor/cap-probe").exists());
    assert!(!guest(&fixture, "srv/velnor/gen/.cap-probe").exists());
    Ok(())
}

#[test]
fn rust_runner_returns_the_unprivileged_guest_user() -> Check {
    let fixture = Fixture::ready()?;
    let user = fixture.launcher().ensure_machine()?;
    assert_eq!(
        user,
        GuestUser {
            uid: 1000,
            gid: 1000,
            home: "/srv/sandbox".to_owned()
        }
    );
    for bad in [
        "0,0,/root",
        "1000,0,/srv/x",
        "1000,1000,home",
        "x,1000,/srv/x",
    ] {
        let rooted = Fixture::ready()?;
        rooted.configure(&format!("FAKE_GUEST_USER={bad}\n"))?;
        assert_eq!(
            rooted.launcher().ensure_machine().err(),
            Some(refused(RustRunnerFailure::GuestUser)),
            "{bad}"
        );
    }
    assert_eq!(fixture.launcher().job_limit_gib(), 64);
    assert_eq!(
        fixture.launcher().with_machine_disk(8, 9).job_limit_gib(),
        9
    );
    Ok(())
}

#[test]
fn rust_runner_sweep_empties_the_job_dirs_inside_the_machine() -> Check {
    let fixture = Fixture::ready()?;
    fixture.launcher().ensure_machine()?;
    for dir in ["work", "stage"] {
        let locked = guest(&fixture, &format!("srv/velnor/gen/{dir}/9/locked"));
        std::fs::create_dir_all(locked.join("inner"))?;
        std::fs::write(locked.join("keep"), "")?;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500))?;
        std::fs::write(
            guest(&fixture, &format!("srv/velnor/gen/{dir}/.hidden")),
            "",
        )?;
    }
    fixture.launcher().sweep()?;
    for dir in ["work", "stage"] {
        let left = std::fs::read_dir(guest(&fixture, &format!("srv/velnor/gen/{dir}")))?.count();
        assert_eq!(left, 0, "{dir}");
        assert!(guest(&fixture, &format!("srv/velnor/gen/{dir}")).is_dir());
    }
    Ok(())
}
