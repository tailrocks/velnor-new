// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
use velnor_runner_host::{HostError, RustRunnerFailure, RustRunnerLauncher};

use super::support::Fixture;

const READY: &str = r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#;
const CHILD: &str = "VELNOR_TEST_STORE_CHILD";

type Check = Result<(), Box<dyn Error>>;

fn prepared() -> Result<Fixture, Box<dyn Error>> {
    let fixture = Fixture::new(READY)?;
    fixture.launcher().ensure_machine()?;
    Ok(fixture)
}

fn nest(work: &Path, levels: usize) -> Check {
    let top = work.join("7");
    std::fs::create_dir(&top)?;
    std::env::set_current_dir(&top)?;
    for _ in 0..levels {
        std::fs::create_dir("d")?;
        std::env::set_current_dir("d")?;
    }
    std::env::set_current_dir("/")?;
    Ok(())
}

pub(super) fn deep_tree_under_a_small_descriptor_limit() -> Check {
    let fixture = prepared()?;
    let limit = getrlimit(Resource::Nofile);
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(256),
            maximum: limit.maximum,
        },
    )?;
    nest(&fixture.store.join("work"), 2_000)?;
    fixture
        .launcher()
        .sweep()
        .map_err(|error| format!("a 2,000 level work tree failed the sweep: {error}"))?;
    assert_eq!(std::fs::read_dir(fixture.store.join("work"))?.count(), 0);
    Ok(())
}

pub(super) fn directories_the_owner_cannot_open() -> Check {
    for mode in [0o500, 0o300, 0o100, 0o000] {
        let fixture = prepared()?;
        let locked = fixture.store.join("work/9/locked");
        std::fs::create_dir_all(locked.join("inner"))?;
        std::fs::write(locked.join("keep"), "")?;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(mode))?;
        let outcome = fixture.launcher().sweep();
        if locked.exists() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700))?;
        }
        outcome.map_err(|error| format!("a {mode:o} directory failed the sweep: {error}"))?;
        assert_eq!(
            std::fs::read_dir(fixture.store.join("work"))?.count(),
            0,
            "a {mode:o} directory was left in work"
        );
    }
    Ok(())
}

pub(super) fn orb_output_over_the_capture_bound() -> Check {
    let fixture = prepared()?;
    fixture.configure("FAKE_ORB_PS_ENDLESS=1\n")?;
    let started = Instant::now();
    assert_eq!(
        fixture
            .launcher()
            .with_orb_deadline(Duration::from_secs(20))
            .list_processes()
            .err(),
        Some(HostError::RustRunner(RustRunnerFailure::OrbOutputTooLarge)),
        "an orb call may not fill host memory with its output"
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "an endless orb output kept the host reading for {:?}",
        started.elapsed()
    );
    Ok(())
}

pub(super) fn one_controller_per_host_store() -> Check {
    let fixture = prepared()?;
    let first = fixture.launcher().hold_host()?;
    assert_eq!(
        fixture.launcher().hold_host().err(),
        Some(HostError::RustRunner(RustRunnerFailure::HostStoreHeld)),
        "a second holder of the same host store"
    );
    drop(first);
    fixture.launcher().hold_host()?;
    Ok(())
}

pub(super) fn ignores_a_store_override_in_the_environment(test: &str) -> Check {
    if std::env::var_os(CHILD).is_some() {
        println!(
            "STORE={}",
            RustRunnerLauncher::from_environment()
                .host_store()
                .display()
        );
        return Ok(());
    }
    let output = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", test, "--nocapture"])
        .env(CHILD, "1")
        .env("VELNOR_HOST_STORE", "/var/tmp/an-override")
        .output()?;
    let reported = String::from_utf8(output.stdout)?;
    assert!(output.status.success(), "{reported}");
    let production = Path::new("/").join("Users/Shared/velnor");
    assert!(
        reported
            .lines()
            .any(|line| line == format!("STORE={}", production.display())),
        "the production store followed an environment override: {reported}"
    );
    Ok(())
}
