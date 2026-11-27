// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde_json::json;
use velnor_runner_host::{HostError, RustRunnerFailure};

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

fn mode(path: &PathBuf) -> Result<u32, Box<dyn Error>> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o7777)
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
    for name in ["jit", "trusted"] {
        assert!(fixture.store.join(name).is_dir(), "{name}");
    }
    assert!(!fixture.store.join("bin").exists());
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
fn rust_runner_compares_the_canonical_share_path() -> Check {
    for listed_through_alias in [true, false] {
        let fixture = Fixture::ready()?;
        let alias = fixture.root.join("alias");
        std::os::unix::fs::symlink(&fixture.root, &alias)?;
        let through_alias = alias.join("store");
        let (listed, used) = if listed_through_alias {
            (&through_alias, &fixture.store)
        } else {
            (&fixture.store, &through_alias)
        };
        fixture.list_share(listed)?;
        let launcher = velnor_runner_host::RustRunnerLauncher::new(
            fixture.root.join("orb"),
            used.clone(),
            fixture.root.join("host-runner"),
        );
        launcher.ensure_machine()?;
        assert!(
            fixture
                .log()?
                .iter()
                .all(|line| !line.starts_with("create")),
            "{listed_through_alias}"
        );
    }
    let other = Fixture::ready()?;
    other.list_share(&other.root.join("elsewhere"))?;
    assert_eq!(
        other.launcher().ensure_machine().err(),
        Some(refused(RustRunnerFailure::MachineShape))
    );
    Ok(())
}

#[test]
fn rust_runner_refuses_a_store_directory_owned_by_another_user_and_leaves_it() -> Check {
    let fixture = Fixture::ready()?;
    fixture.launcher().ensure_machine()?;
    let installs = fixture.commands("install")?;
    fixture.configure("FAKE_GUEST_OWNERS=/var/lib/velnor/cache=1000\n")?;
    let error = fixture
        .launcher()
        .ensure_machine()
        .err()
        .ok_or("accepted")?;
    assert!(
        matches!(
            error,
            HostError::RustRunner(RustRunnerFailure::StoreOwner(_))
        ),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("/var/lib/velnor/cache"),
        "{error}"
    );
    assert_eq!(
        fixture.commands("install")?,
        installs,
        "the directory was re-owned"
    );
    Ok(())
}

#[test]
fn rust_runner_provisions_the_vm_local_store_layout() -> Check {
    let fixture = Fixture::ready()?;
    std::fs::write(fixture.root.join("host-runner"), "runner-build")?;
    fixture.launcher().ensure_machine()?;
    fixture.launcher().ensure_machine()?;
    for path in OPEN {
        assert!(fixture.guest(path).is_dir(), "{path}");
        assert_eq!(mode(&fixture.guest(path))?, 0o755, "{path}");
    }
    for path in CLOSED {
        assert_eq!(mode(&fixture.guest(path))?, 0o700, "{path}");
    }
    let copy = fixture.guest("var/lib/velnor/bin/velnor-runner");
    assert_eq!(std::fs::read_to_string(&copy)?, "runner-build");
    assert_eq!(mode(&copy)?, 0o755);
    assert_eq!(fixture.commands("ln")?, 0);
    assert!(!fixture.guest("var/lib/velnor/bin/.stage").exists());
    assert!(!fixture.store.join("bin").exists());
    Ok(())
}

#[test]
fn rust_runner_refuses_an_ancestor_that_the_sandbox_user_could_own() -> Check {
    for owned in ["/var/lib", "/srv"] {
        let fixture = Fixture::ready()?;
        fixture.configure(&format!("FAKE_GUEST_OWNERS={owned}=1000\n"))?;
        fixture.ensure_refused(RustRunnerFailure::MachineLayout);
    }
    let writable = Fixture::ready()?;
    writable.configure("FAKE_GUEST_MODES=/var=777\n")?;
    writable.ensure_refused(RustRunnerFailure::MachineLayout);
    Ok(())
}

#[test]
fn rust_runner_refuses_stores_that_are_not_on_btrfs() -> Check {
    let fixture = Fixture::ready()?;
    fixture.configure("FAKE_GUEST_FS=xfs\n")?;
    fixture.ensure_refused(RustRunnerFailure::MachineFilesystem);
    Ok(())
}

#[test]
fn rust_runner_refuses_a_machine_without_protected_links() -> Check {
    for values in ["1,0", "0,1", "0,0"] {
        let fixture = Fixture::ready()?;
        fixture.configure(&format!("FAKE_GUEST_SYSCTL={values}\n"))?;
        fixture.ensure_refused(RustRunnerFailure::MachineShape);
    }
    Ok(())
}

#[test]
fn rust_runner_probes_the_disk_cap_once_per_orbstack_version() -> Check {
    let fixture = Fixture::ready()?;
    let record = fixture.guest("var/lib/velnor/cap-probe");
    fixture.launcher().ensure_machine()?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(fixture.commands("fallocate")?, 1);
    assert_eq!(
        std::fs::read_to_string(&record)?.trim(),
        format!("orbstack=2.2.5-2020500 cap={CAP_BYTES}")
    );
    fixture.configure("FAKE_ORB_VERSION=2.3.0 (2030000)\n")?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(fixture.commands("fallocate")?, 2);
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
    fixture.ensure_refused(RustRunnerFailure::CapProbe);
    assert!(!fixture.guest("var/lib/velnor/cap-probe").exists());
    assert!(!fixture.guest("var/lib/velnor/.cap-probe").exists());
    Ok(())
}

#[test]
fn rust_runner_takes_only_a_quota_error_as_proof_of_the_disk_cap() -> Check {
    for error in ["Operation not supported", "Input/output error"] {
        let fixture = Fixture::ready()?;
        fixture.configure(&format!("FAKE_GUEST_CAP_ERROR={error}\n"))?;
        fixture.ensure_refused(RustRunnerFailure::CapProbe);
        assert!(
            !fixture.guest("var/lib/velnor/cap-probe").exists(),
            "{error}"
        );
    }
    for error in ["No space left on device", "Disk quota exceeded"] {
        let fixture = Fixture::ready()?;
        fixture.configure(&format!("FAKE_GUEST_CAP_ERROR={error}\n"))?;
        fixture.launcher().ensure_machine()?;
        assert!(
            fixture.guest("var/lib/velnor/cap-probe").exists(),
            "{error}"
        );
    }
    Ok(())
}

#[test]
fn rust_runner_job_limit_follows_the_configured_machine_disk() -> Check {
    let fixture = Fixture::ready()?;
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
        let locked = fixture.guest(&format!("srv/velnor/gen/{dir}/9/locked"));
        std::fs::create_dir_all(locked.join("inner"))?;
        std::fs::write(locked.join("keep"), "")?;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500))?;
        std::fs::write(fixture.guest(&format!("srv/velnor/gen/{dir}/.hidden")), "")?;
    }
    fixture.launcher().sweep()?;
    for dir in ["work", "stage"] {
        let left = std::fs::read_dir(fixture.guest(&format!("srv/velnor/gen/{dir}")))?.count();
        assert_eq!(left, 0, "{dir}");
        assert!(fixture.guest(&format!("srv/velnor/gen/{dir}")).is_dir());
    }
    Ok(())
}

#[test]
fn rust_runner_sweep_refuses_job_dirs_it_cannot_empty() -> Check {
    let fixture = Fixture::ready()?;
    fixture.launcher().ensure_machine()?;
    let planted = fixture.guest("srv/velnor/gen/work/9/data");
    std::fs::create_dir_all(&planted)?;
    fixture.configure("FAKE_GUEST_WEDGE=1\n")?;
    assert_eq!(
        fixture.launcher().sweep().err(),
        Some(refused(RustRunnerFailure::JobDirsWedged))
    );
    assert!(planted.exists());
    fixture.configure("")?;
    fixture.launcher().sweep()?;
    assert!(!planted.exists());
    Ok(())
}
