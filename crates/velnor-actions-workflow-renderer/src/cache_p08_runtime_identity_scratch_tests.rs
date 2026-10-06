use std::{
    error::Error,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Child, ExitStatus},
    thread,
    time::{Duration, Instant},
};

use super::{Fixture, assert_disabled, identity, identity_command, payload};

fn install_hash_stub(fixture: &Fixture, script: &str) -> Result<String, Box<dyn Error>> {
    let bin = fixture.root.join("bin");
    fs::create_dir_all(&bin)?;
    let executable = bin.join("sha256sum");
    fs::write(&executable, format!("#!/bin/sh\n{script}\n"))?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    Ok(std::env::join_paths(paths)?.to_string_lossy().into_owned())
}

fn assert_scratch_removed(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let scratch_root = fixture.runner_temp.join("velnor");
    if scratch_root.exists() {
        for entry in fs::read_dir(scratch_root)? {
            let name = entry?.file_name();
            assert!(
                !name.to_string_lossy().starts_with("runtime-identity."),
                "identity scratch directory leaked: {name:?}"
            );
        }
    }
    Ok(())
}

fn wait_for_marker(child: &mut Child, marker: &Path) -> Result<(), Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if marker.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(format!("hash stub exited before signaling: {status}").into());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait()?;
            return Err("timed out waiting for hash stub".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_child(child: &mut Child) -> Result<ExitStatus, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let status = child.wait()?;
            return Err(format!("identity script did not stop after TERM: {status}").into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn existing_final_symlinks_cannot_redirect_identity_or_digest_writes() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new()?;
    let cache_root = fixture.runner_temp.join("velnor");
    fs::create_dir_all(&cache_root)?;
    let preimage_target = fixture.root.join("external preimage");
    let digest_target = fixture.root.join("external digest");
    fs::write(&preimage_target, "preimage sentinel\n")?;
    fs::write(&digest_target, "digest sentinel\n")?;
    let preimage_link = cache_root.join("tool-cache-identity");
    let digest_link = cache_root.join("tool-cache-identity-sum");
    symlink(&preimage_target, &preimage_link)?;
    symlink(&digest_target, &digest_link)?;

    let (output, text) = super::run_and_read(&payload("ubuntu-26.04"), &fixture, &[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        text.lines().find(|line| line.starts_with("enabled=")),
        Some("enabled=true")
    );
    assert_eq!(fs::read_to_string(&preimage_target)?, "preimage sentinel\n");
    assert_eq!(fs::read_to_string(&digest_target)?, "digest sentinel\n");
    assert!(
        fs::symlink_metadata(&preimage_link)?
            .file_type()
            .is_symlink()
    );
    assert!(fs::symlink_metadata(&digest_link)?.file_type().is_symlink());
    assert_eq!(identity(&text).len(), 64);
    assert_scratch_removed(&fixture)?;
    Ok(())
}

#[test]
fn hash_failure_disables_identity_and_removes_private_scratch() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let path = install_hash_stub(&fixture, "exit 19")?;
    let (output, text) = super::run_and_read(&payload("ubuntu-26.04"), &fixture, &[("PATH", path)]);
    assert_disabled(&output, &text, "identity_hash_failed");
    assert_scratch_removed(&fixture)?;
    Ok(())
}

#[test]
fn cancellation_during_hashing_cleans_private_scratch_without_enabling_cache()
-> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let path = install_hash_stub(
        &fixture,
        "printf started > \"$IDENTITY_HASH_STARTED\"\nkill -TERM \"$PPID\"\nsleep 1\nprintf '%064d  -\\n' 0",
    )?;
    let marker = fixture.root.join("hash started");
    let mut command = identity_command(
        &payload("ubuntu-26.04"),
        &fixture,
        &[
            ("PATH", path),
            (
                "IDENTITY_HASH_STARTED",
                marker.to_string_lossy().into_owned(),
            ),
        ],
        &[],
    );
    let mut child = command.spawn()?;
    wait_for_marker(&mut child, &marker)?;
    let status = wait_for_child(&mut child)?;
    assert_eq!(status.code(), Some(143), "{status}");
    let output = fixture.output()?;
    assert!(
        !output.lines().any(|line| line == "enabled=true"),
        "{output}"
    );
    assert_scratch_removed(&fixture)?;
    Ok(())
}
