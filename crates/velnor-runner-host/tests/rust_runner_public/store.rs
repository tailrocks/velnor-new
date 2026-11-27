// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_runner_host::{HostError, RustRunnerFailure, RustRunnerLauncher};

use super::support::{Fixture, SECRET_JIT, jit, reservation};

const READY: &str = r#"[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]"#;

type Check = Result<(), Box<dyn Error>>;

const fn refused(failure: RustRunnerFailure) -> HostError {
    HostError::RustRunner(failure)
}

fn fixture() -> Result<Fixture, Box<dyn Error>> {
    Fixture::new(READY)
}

fn chmod(path: &Path, mode: u32) -> Check {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn add_acl(path: &Path) -> Check {
    let status = if cfg!(target_os = "macos") {
        Command::new("/bin/chmod")
            .args(["+a", "everyone allow read"])
            .arg(path)
            .status()?
    } else {
        Command::new("setfacl")
            .args(["-m", "u:nobody:r"])
            .arg(path)
            .status()?
    };
    assert!(status.success(), "the test needs a tool that sets an ACL");
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
    let foreign = fixture()?;
    let launcher = RustRunnerLauncher::new(foreign.root.join("orb"), PathBuf::from("/usr"));
    assert_eq!(
        launcher.ensure_machine().err(),
        Some(refused(RustRunnerFailure::StoreUnsafe))
    );
    Ok(())
}

fn unsafe_binaries() -> Check {
    for mode in [0o775, 0o644, 0o757] {
        let fixture = fixture()?;
        chmod(&fixture.store.join("bin/velnor-runner"), mode)?;
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
    let binary = linked.store.join("bin/velnor-runner");
    std::fs::remove_file(&binary)?;
    symlink(&target, &binary)?;
    assert_refused(&linked, RustRunnerFailure::BinaryUnsafe, "a symlink")?;
    for (name, expected) in [
        ("bin", RustRunnerFailure::StoreUnsafe),
        ("bin/velnor-runner", RustRunnerFailure::StoreUnsafe),
    ] {
        let fixture = fixture()?;
        add_acl(&fixture.store.join(name))?;
        assert_refused(&fixture, expected, &format!("an ACL on {name}"))?;
    }
    Ok(())
}

fn symlinked_directories() -> Check {
    let fixture = fixture()?;
    let victim = fixture.root.join("victim");
    std::fs::create_dir(&victim)?;
    chmod(&victim, 0o700)?;
    symlink(&victim, fixture.store.join("work"))?;
    assert!(fixture.launcher().ensure_machine().is_err());
    assert_eq!(std::fs::read_dir(&victim)?.count(), 0);
    for name in ["work", "jit"] {
        let fixture = fixture_with_directories()?;
        let victim = fixture.root.join("victim");
        std::fs::create_dir(&victim)?;
        std::fs::write(victim.join("keep"), "")?;
        let directory = fixture.store.join(name);
        std::fs::remove_dir(&directory)?;
        symlink(&victim, &directory)?;
        assert!(fixture.launcher().sweep().is_err(), "{name}");
        assert!(victim.join("keep").exists(), "{name}");
    }
    Ok(())
}

fn fixture_with_directories() -> Result<Fixture, Box<dyn Error>> {
    let fixture = fixture()?;
    fixture.launcher().ensure_machine()?;
    Ok(fixture)
}

async fn acl_after_a_started_job() -> Check {
    let fixture = fixture_with_directories()?;
    let launcher = fixture.launcher();
    let mut job = launcher.start_job(reservation(51)?, &jit(SECRET_JIT))?;
    assert!(job.wait().await?.success());
    add_acl(&fixture.store.join("bin/velnor-runner"))?;
    assert_eq!(
        launcher.start_job(reservation(52)?, &jit(SECRET_JIT)).err(),
        Some(refused(RustRunnerFailure::StoreUnsafe))
    );
    Ok(())
}

fn sweep_without_init() -> Check {
    let fixture = fixture_with_directories()?;
    let planted = fixture.store.join("jit/99.jit");
    std::fs::write(&planted, "stale")?;
    chmod(&planted, 0o600)?;
    std::fs::write(fixture.root.join("ps.table"), "")?;
    fixture.configure("FAKE_ORB_PS_NO_INIT=1\n")?;
    assert_eq!(
        fixture.launcher().sweep().err(),
        Some(refused(RustRunnerFailure::ProcessList))
    );
    assert!(planted.exists());
    fixture.configure("")?;
    fixture.launcher().sweep()?;
    assert!(!planted.exists());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn rust_runner_refuses_an_unsafe_host_store() -> Check {
    unsafe_roots()?;
    unsafe_binaries()?;
    symlinked_directories()?;
    acl_after_a_started_job().await?;
    sweep_without_init()
}
