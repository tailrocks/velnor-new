// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::PermissionsExt;

use velnor_runner_host::{HostError, RustRunnerFailure};

use super::support::{Fixture, GUEST, SECRET_JIT, jit, reservation};

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
    let log = fixture.log()?;
    let stage = format!("{GUEST} chmod 0755 -- /var/lib/velnor/bin/.stage");
    let rename =
        format!("{GUEST} mv -fT -- /var/lib/velnor/bin/.stage /var/lib/velnor/bin/velnor-runner");
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

#[test]
fn rust_runner_refuses_a_runner_path_that_is_a_directory_and_keeps_the_stage_out_of_it() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "first-build")?;
    fixture.launcher().ensure_machine()?;
    std::fs::remove_file(fixture.guest(INSTALLED))?;
    std::fs::create_dir(fixture.guest(INSTALLED))?;
    build(&fixture, "second-build")?;
    assert_eq!(
        fixture.launcher().ensure_machine().err(),
        Some(HostError::RustRunner(RustRunnerFailure::MachineProvision))
    );
    assert!(fixture.guest(INSTALLED).is_dir());
    assert_eq!(std::fs::read_dir(fixture.guest(INSTALLED))?.count(), 0);
    assert!(!fixture.guest(STAGE).exists());
    Ok(())
}

#[test]
fn rust_runner_replaces_a_runner_path_that_is_a_link_to_a_directory() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "first-build")?;
    fixture.launcher().ensure_machine()?;
    let elsewhere = fixture.root.join("elsewhere");
    std::fs::create_dir(&elsewhere)?;
    std::fs::remove_file(fixture.guest(INSTALLED))?;
    std::os::unix::fs::symlink(&elsewhere, fixture.guest(INSTALLED))?;
    build(&fixture, "second-build")?;
    fixture.launcher().ensure_machine()?;
    assert_eq!(text(&fixture, INSTALLED)?, "second-build");
    assert!(
        !std::fs::symlink_metadata(fixture.guest(INSTALLED))?
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_dir(&elsewhere)?.count(), 0);
    Ok(())
}

#[test]
fn rust_runner_checks_the_installed_runner_after_the_rename_and_removes_a_wrong_one() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "host-build")?;
    fixture.configure("FAKE_GUEST_SWAP_STAGE=1\n")?;
    fixture.ensure_refused(RustRunnerFailure::BinaryCopy);
    assert!(!fixture.guest(INSTALLED).exists());
    assert!(!fixture.guest(STAGE).exists());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_records_the_digest_of_the_runner_copy_in_the_machine() -> Check {
    let fixture = Fixture::ready()?;
    build(&fixture, "installed-build")?;
    let launcher = fixture.launcher();
    launcher.ensure_machine()?;
    build(&fixture, "replaced-on-the-host")?;
    let installed = hex_sha256(b"installed-build");
    assert_eq!(launcher.installed_runner_digest()?, installed);
    assert_eq!(
        launcher.runner_digest()?,
        hex_sha256(b"replaced-on-the-host")
    );
    let mut job = launcher.start_job(reservation(71)?, &jit(SECRET_JIT))?;
    assert_eq!(job.binary(), installed);
    assert!(job.wait().await?.success());
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    use std::fmt::Write;

    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}
