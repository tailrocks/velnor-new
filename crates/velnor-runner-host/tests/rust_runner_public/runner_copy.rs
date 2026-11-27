// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::RustRunnerFailure;

use super::support::{Fixture, GUEST};

type Check = Result<(), Box<dyn Error>>;

const INSTALLED: &str = "var/lib/velnor/bin/velnor-runner";
const STAGE: &str = "var/lib/velnor/bin/.stage";

fn build(fixture: &Fixture, text: &str) -> Check {
    std::fs::write(fixture.root.join("host-runner"), text)?;
    Ok(())
}

fn text(fixture: &Fixture, path: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(fixture.guest(path))?)
}

#[test]
fn rust_runner_streams_its_own_runner_into_a_root_stage_and_renames_it() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "host-build")?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(text(&fixture, INSTALLED)?, "host-build");
    let mode = std::fs::metadata(fixture.guest(INSTALLED))?
        .permissions()
        .mode();
    assert_eq!(mode & 0o7777, 0o755);
    assert!(!fixture.guest(STAGE).exists());
    assert!(!fixture.store.join("bin").exists());
    let log = fixture.log()?;
    let stage = format!("{GUEST} chmod 0755 -- /var/lib/velnor/bin/.stage");
    let rename =
        format!("{GUEST} mv -f -- /var/lib/velnor/bin/.stage /var/lib/velnor/bin/velnor-runner");
    let at = |wanted: &str| log.iter().position(|line| line.starts_with(wanted));
    let hashed = at(&format!("{GUEST} sha256sum /var/lib/velnor/bin/.stage"));
    assert!(hashed < at(&stage) && at(&stage) < at(&rename), "{log:?}");
    assert!(hashed.is_some());

    fixture.launcher().ensure_machine()?;
    assert_eq!(
        fixture.commands("umask")?,
        1,
        "an unchanged runner was copied again"
    );
    build(&fixture, "next-build")?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(text(&fixture, INSTALLED)?, "next-build");
    assert_eq!(fixture.commands("umask")?, 2);
    Ok(())
}

#[test]
fn rust_runner_reads_nothing_from_the_share_for_the_runner() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "host-build")?;
    let elsewhere = fixture.root.join("elsewhere");
    std::fs::create_dir(&elsewhere)?;
    std::fs::write(elsewhere.join("velnor-runner"), "tampered")?;
    std::os::unix::fs::symlink(&elsewhere, fixture.store.join("bin"))?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(text(&fixture, INSTALLED)?, "host-build");
    assert!(
        fixture
            .log()?
            .iter()
            .all(|line| !line.contains("/var/lib/velnor-host/bin")),
        "{:?}",
        fixture.log()?
    );
    Ok(())
}

#[test]
fn rust_runner_refuses_a_stream_that_does_not_arrive_as_the_host_runner() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "host-build")?;
    fixture.configure("FAKE_GUEST_CORRUPT_STREAM=1\n")?;
    fixture.ensure_refused(RustRunnerFailure::BinaryCopy);
    assert!(!fixture.guest(STAGE).exists(), "the stage was left behind");
    Ok(())
}

#[test]
fn rust_runner_leaves_nothing_behind_when_the_stage_cannot_be_written() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "host-build")?;
    fixture.configure("FAKE_GUEST_STREAM_FAIL=1\n")?;
    fixture.ensure_refused(RustRunnerFailure::MachineProvision);
    assert!(!fixture.guest(STAGE).exists(), "the stage was left behind");
    Ok(())
}

#[test]
fn rust_runner_keeps_the_installed_runner_when_a_refresh_fails() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "first-build")?;
    fixture.launcher().ensure_machine()?;
    build(&fixture, "second-build")?;
    fixture.configure("FAKE_GUEST_CORRUPT_STREAM=1\n")?;
    assert!(fixture.launcher().ensure_machine().is_err());
    assert_eq!(text(&fixture, INSTALLED)?, "first-build");
    assert!(!fixture.guest(STAGE).exists());
    Ok(())
}
