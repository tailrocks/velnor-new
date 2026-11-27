// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

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

pub(super) fn orb_output_over_the_capture_bound() -> Check {
    let fixture = prepared()?;
    let line = format!("4242 {}\n", "x".repeat(1_000));
    std::fs::write(
        fixture.root.join("ps.table"),
        line.repeat(9 * 1024 * 1024 / line.len() + 1),
    )?;
    assert_eq!(
        fixture.launcher().list_processes().err(),
        Some(HostError::RustRunner(RustRunnerFailure::OrbOutputTooLarge)),
        "an orb call may not fill host memory with its output"
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
