// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use super::setup_action::{ARCHIVE, Install, TestResult, program};

fn read_only_cache_installs_privately(install: &Install, leaf: &str) -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let cache = install.work.join(leaf.split('/').next().unwrap_or(leaf));
    let locked = install.work.join(leaf);
    fs::create_dir_all(&locked)?;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555))?;
    if fs::write(locked.join("probe"), b"").is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))?;
        return Ok(());
    }
    let status = install.run(Some(&cache), None)?;
    let copy = install.installed();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))?;
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert_eq!(copy?, install.binary);
    Ok(())
}

#[test]
fn an_unset_tool_cache_does_not_stop_the_private_install() -> TestResult {
    let install = Install::new("cache-unset")?;
    let status = install.run(None, None)?;
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert_eq!(copy?, install.binary);
    Ok(())
}

#[test]
fn a_read_only_tool_cache_does_not_stop_the_private_install() -> TestResult {
    read_only_cache_installs_privately(&Install::new("cache-read-only")?, "read-only")
}

#[test]
fn a_read_only_cache_entry_directory_does_not_stop_the_private_install() -> TestResult {
    read_only_cache_installs_privately(
        &Install::new("cache-read-only-leaf")?,
        "read-only-leaf/velnor-ci/1.2.3/x64",
    )
}

pub(crate) fn seed_entry(
    install: &Install,
) -> Result<(std::path::PathBuf, std::path::PathBuf), Box<dyn Error>> {
    let cache = install.work.join("cache");
    let entry = cache.join("velnor-ci/1.2.3/x64");
    fs::create_dir_all(&entry)?;
    Ok((cache, entry.join(ARCHIVE)))
}

#[test]
fn a_cache_entry_that_is_not_a_regular_file_is_ignored_instead_of_read() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    for kind in ["fifo", "symlink-to-fifo"] {
        let install = Install::new(&format!("cache-{kind}"))?;
        let (cache, entry) = seed_entry(&install)?;
        let fifo = if kind == "fifo" {
            entry.clone()
        } else {
            install.work.join("elsewhere")
        };
        assert!(
            Command::new(program("mkfifo"))
                .arg(&fifo)
                .status()?
                .success()
        );
        fs::set_permissions(&fifo, fs::Permissions::from_mode(0o755))?;
        if kind != "fifo" {
            symlink(&fifo, &entry)?;
        }
        let mut child = install.spawn(Some(&cache), |command| {
            command.process_group(0);
        })?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if std::time::Instant::now() > deadline {
                let group = child.id().to_string();
                Command::new(program("kill"))
                    .args(["-KILL", "--", &format!("-{group}")])
                    .status()?;
                child.wait()?;
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        let copy = install.installed();
        fs::remove_dir_all(&install.work)?;
        assert!(
            status.is_some_and(|status| status.success()),
            "{kind}: the install hung or failed"
        );
        assert_eq!(copy?, install.binary, "{kind}");
    }
    Ok(())
}

#[test]
fn a_tampered_cache_entry_is_rejected_by_the_sha256_and_replaced_by_a_download() -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let install = Install::new("cache-tampered")?;
    let (cache, entry) = seed_entry(&install)?;
    fs::write(&entry, b"tampered archive\n")?;
    fs::set_permissions(&entry, fs::Permissions::from_mode(0o755))?;
    let archive = fs::read(install.archive_path())?;
    let status = install.run(Some(&cache), None)?;
    let copy = install.installed();
    let republished = fs::read(&entry);
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert_eq!(copy?, install.binary);
    assert_eq!(republished?, archive);
    Ok(())
}

#[test]
fn a_failed_copy_into_the_tool_cache_publishes_no_entry() -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    let install = Install::new("cache-copy-fails")?;
    let shims = install.work.join("shims");
    fs::create_dir_all(&shims)?;
    let cp = shims.join("cp");
    fs::write(
        &cp,
        "#!/bin/sh\ncase \"$2\" in\n  */.velnor-ci.*) head -c 5 \"$1\" > \"$2\"; exit 1 ;;\nesac\nexec /bin/cp \"$@\"\n",
    )?;
    fs::set_permissions(&cp, fs::Permissions::from_mode(0o755))?;
    let cache = install.work.join("cache");
    let status = install.run(Some(&cache), Some(&shims))?;
    let entries: Vec<_> = fs::read_dir(cache.join("velnor-ci/1.2.3/x64"))?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<Result<_, _>>()?;
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(status.success());
    assert!(entries.is_empty(), "{entries:?}");
    assert_eq!(copy?, install.binary);
    Ok(())
}

#[test]
fn a_special_file_in_the_tool_cache_is_not_read_and_the_download_is_used() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let install = Install::new("cache-fifo")?;
    let entry = install.work.join("cache/velnor-ci/1.2.3/x64");
    fs::create_dir_all(&entry)?;
    let fifo = entry.join(ARCHIVE);
    assert!(
        Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()?
            .success()
    );
    fs::set_permissions(&fifo, fs::Permissions::from_mode(0o755))?;
    let status = install.run(Some(&install.work.join("cache")), None);
    let copy = install.installed();
    fs::remove_dir_all(&install.work)?;
    assert!(status?.success());
    assert_eq!(copy?, install.binary);
    Ok(())
}
