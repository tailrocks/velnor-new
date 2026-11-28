// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_runner_host::{HostError, RustRunnerFailure, RustRunnerLauncher};

use super::support::Fixture;

type Check = Result<(), Box<dyn Error>>;

const fn refused(failure: RustRunnerFailure) -> HostError {
    HostError::RustRunner(failure)
}

fn fixture() -> Result<Fixture, Box<dyn Error>> {
    Fixture::ready()
}

fn chmod(path: &Path, mode: u32) -> Check {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn add_acl(path: &Path) -> Check {
    let status = Command::new("/bin/chmod")
        .args(["+a", "everyone allow read"])
        .arg(path)
        .status()?;
    assert!(
        status.success(),
        "chmod +a could not set an ACL on {}",
        path.display()
    );
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn add_acl(path: &Path) -> Check {
    const VERSION: u32 = 2;
    const USER_OBJ: u16 = 0x01;
    const USER: u16 = 0x02;
    const GROUP_OBJ: u16 = 0x04;
    const MASK: u16 = 0x10;
    const OTHER: u16 = 0x20;
    const NOBODY: u32 = 65_534;
    let mode = std::fs::metadata(path)?.permissions().mode();
    let perm = |shift: u32| u16::try_from((mode >> shift) & 0o7).unwrap_or(0);
    let mut value = VERSION.to_le_bytes().to_vec();
    for (tag, permission, id) in [
        (USER_OBJ, perm(6), u32::MAX),
        (USER, 4, NOBODY),
        (GROUP_OBJ, perm(3), u32::MAX),
        (MASK, perm(3) | 4, u32::MAX),
        (OTHER, perm(0), u32::MAX),
    ] {
        value.extend_from_slice(&tag.to_le_bytes());
        value.extend_from_slice(&permission.to_le_bytes());
        value.extend_from_slice(&id.to_le_bytes());
    }
    rustix::fs::setxattr(
        path,
        "system.posix_acl_access",
        &value,
        rustix::fs::XattrFlags::empty(),
    )
    .map_err(|error| {
        format!(
            "the filesystem of {} cannot hold a POSIX ACL for this test: {error}",
            path.display()
        )
    })?;
    Ok(())
}

fn assert_refused(fixture: &Fixture, failure: RustRunnerFailure, what: &str) -> Check {
    assert_eq!(
        fixture.launcher().ensure_machine().err(),
        Some(refused(failure)),
        "{what}"
    );
    assert!(
        fixture.log()?.iter().all(|line| !line.contains(" ln ")),
        "{what}: {:?}",
        fixture.log()?
    );
    Ok(())
}

fn unsafe_roots() -> Check {
    for mode in [0o770, 0o750, 0o705, 0o777] {
        let fixture = fixture()?;
        chmod(&fixture.store, mode)?;
        assert_refused(
            &fixture,
            RustRunnerFailure::StoreUnsafe,
            &format!("{mode:o}"),
        )?;
    }
    let acl = fixture()?;
    add_acl(&acl.store)?;
    assert_refused(&acl, RustRunnerFailure::StoreUnsafe, "an ACL on the root")?;
    let id = Command::new("id").arg("-u").output()?;
    assert_ne!(String::from_utf8_lossy(&id.stdout).trim(), "0");
    let foreign = Fixture::ready()?;
    let launcher = RustRunnerLauncher::new(
        foreign.root.join("orb"),
        PathBuf::from("/usr"),
        foreign.root.join("host-runner"),
    );
    assert_eq!(
        launcher.ensure_machine().err(),
        Some(refused(RustRunnerFailure::StoreUnsafe)),
        "a store owned by another user"
    );
    Ok(())
}

fn unsafe_binaries() -> Check {
    for mode in [0o775, 0o644, 0o757] {
        let fixture = fixture()?;
        chmod(&fixture.root.join("host-runner"), mode)?;
        assert_refused(
            &fixture,
            RustRunnerFailure::BinaryUnsafe,
            &format!("{mode:o}"),
        )?;
    }
    let linked = fixture()?;
    let target = linked.root.join("elsewhere");
    std::fs::write(&target, "")?;
    chmod(&target, 0o755)?;
    let binary = linked.root.join("host-runner");
    std::fs::remove_file(&binary)?;
    symlink(&target, &binary)?;
    assert_refused(&linked, RustRunnerFailure::BinaryUnsafe, "a symlink")?;
    #[cfg(target_os = "macos")]
    root_owned_binary()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn root_owned_binary() -> Check {
    use std::os::unix::fs::MetadataExt;
    const TOOLS: &str = "/Library/Developer/CommandLineTools/usr/bin";
    let source = std::fs::read_dir(TOOLS)
        .map_err(|_| format!("the root-owned binary row needs the command line tools in {TOOLS}"))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            std::fs::symlink_metadata(path).is_ok_and(|meta| {
                meta.is_file()
                    && meta.uid() == 0
                    && meta.mode() & 0o111 != 0
                    && meta.mode() & 0o022 == 0
            })
        })
        .ok_or("no root-owned executable file in the command line tools")?;
    let fixture = fixture()?;
    let binary = fixture.root.join("host-runner");
    std::fs::remove_file(&binary)?;
    std::fs::hard_link(&source, &binary)?;
    assert_refused(
        &fixture,
        RustRunnerFailure::BinaryUnsafe,
        "a root-owned binary hard-linked into the store",
    )
}

fn fixture_with_directories() -> Result<Fixture, Box<dyn Error>> {
    let fixture = fixture()?;
    fixture.launcher().ensure_machine()?;
    Ok(fixture)
}

fn acl_on_the_runner() -> Check {
    let fixture = fixture()?;
    add_acl(&fixture.root.join("host-runner"))?;
    assert_refused(
        &fixture,
        RustRunnerFailure::BinaryUnsafe,
        "an ACL on the runner",
    )
}

fn sweep_without_init() -> Check {
    let fixture = fixture_with_directories()?;
    let planted = fixture.guest("srv/velnor/gen/work/99/stale");
    std::fs::create_dir_all(planted.parent().ok_or("no parent")?)?;
    std::fs::write(&planted, "stale")?;
    std::fs::write(fixture.root.join("ps.table"), "")?;
    fixture.configure("FAKE_ORB_PS_NO_INIT=1\n")?;
    assert_eq!(
        fixture.launcher().sweep().err(),
        Some(refused(RustRunnerFailure::ProcessList))
    );
    assert!(
        planted.exists(),
        "a ps listing without pid 1 must not let the sweep delete files"
    );
    fixture.configure("")?;
    fixture.launcher().sweep()?;
    assert!(
        !planted.exists(),
        "a ps listing with pid 1 must let the sweep delete stale files"
    );
    Ok(())
}

#[test]
fn rust_runner_refuses_an_unsafe_host_store() -> Check {
    unsafe_roots()?;
    unsafe_binaries()?;
    acl_on_the_runner()?;
    sweep_without_init()
}
