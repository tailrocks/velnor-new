// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::fd::AsFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};
use rustix::fs::{CWD, Mode, OFlags, fstat, mkdirat, openat};
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

use super::safe_fs::{directory_flags, open_path, remove_entry};
use super::tests::{Fixture, Scratch};

const HOISTED_DEPTH: usize = 40;

fn deep_tree(parent: &Path, depth: usize) -> Result<()> {
    let mut current = openat(
        CWD,
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    for _ in 0..depth {
        mkdirat(&current, "d", Mode::from_bits_truncate(0o755))?;
        current = openat(
            &current,
            "d",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
    }
    drop(openat(
        &current,
        "leaf",
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o644),
    )?);
    Ok(())
}

struct OpenFileLimit(Rlimit);

impl OpenFileLimit {
    fn lower(current: u64) -> Result<Self> {
        let before = getrlimit(Resource::Nofile);
        setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(current),
                maximum: before.maximum,
            },
        )?;
        Ok(Self(before))
    }
}

impl Drop for OpenFileLimit {
    fn drop(&mut self) {
        let _ = setrlimit(Resource::Nofile, self.0);
    }
}

struct Undeletable(PathBuf);

impl Undeletable {
    fn pin(root: &Path, file: &Path) -> Result<Self> {
        let status = if cfg!(target_os = "macos") {
            Command::new("chflags").arg("uchg").arg(file).status()
        } else {
            Command::new("chattr").arg("+i").arg(file).status()
        }
        .context("run the file-pinning command")?;
        ensure!(
            status.success(),
            "pinning {} failed: {status}",
            file.display()
        );
        Ok(Self(root.to_owned()))
    }
}

impl Drop for Undeletable {
    fn drop(&mut self) {
        let _ = if cfg!(target_os = "macos") {
            Command::new("chflags")
                .args(["-R", "nouchg"])
                .arg(&self.0)
                .status()
        } else {
            Command::new("chattr")
                .args(["-R", "-i"])
                .arg(&self.0)
                .status()
        };
    }
}

fn names_in(directory: &Path) -> Vec<OsString> {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|item| item.file_name())
        .collect()
}

#[test]
fn dropping_the_broker_removes_a_read_only_and_a_very_deep_stage_tree() -> Result<()> {
    let fixture = Fixture::new()?;
    let dir = fixture.upload_dir()?;
    let locked = dir.join("locked");
    fs::create_dir(&locked)?;
    fs::write(locked.join("file"), b"x")?;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555))?;
    let nested = dir.join("nested");
    fs::create_dir(&nested)?;
    deep_tree(&nested, 600)?;
    let stages = dir.parent().context("stage parent")?.to_path_buf();
    let Fixture {
        scratch, broker, ..
    } = fixture;
    let before = getrlimit(Resource::Nofile);
    let limit = OpenFileLimit::lower(128)?;
    drop(broker);
    drop(limit);
    assert_eq!(getrlimit(Resource::Nofile), before);
    assert!(!stages.exists());
    let remaining: Vec<PathBuf> = fs::read_dir(scratch.0.join("store"))?
        .flatten()
        .map(|item| item.path())
        .filter(|path| {
            path.is_dir() && fs::read_dir(path).is_ok_and(|mut items| items.next().is_some())
        })
        .collect();
    assert_eq!(remaining, Vec::<PathBuf>::new());
    Ok(())
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "needs chflags uchg to make the purge fail"
)]
fn a_failed_purge_leaves_only_what_belongs_to_this_broker() -> Result<()> {
    let fixture = Fixture::new()?;
    let dir = fixture.upload_dir()?;
    deep_tree(&dir, HOISTED_DEPTH)?;
    let leaf = (0..HOISTED_DEPTH)
        .fold(dir.clone(), |path, _| path.join("d"))
        .join("leaf");
    let id = dir
        .parent()
        .and_then(Path::file_name)
        .context("stage id")?
        .to_owned();
    let Fixture {
        scratch, broker, ..
    } = fixture;
    let _pinned = Undeletable::pin(&scratch.0, &leaf)?;
    drop(broker);
    let store = scratch.0.join("store");
    assert_eq!(names_in(&store.join("incoming")), std::slice::from_ref(&id));
    assert!(
        names_in(&store.join("aside"))
            .iter()
            .all(|name| *name == id)
    );
    assert!(
        store.join("incoming").join(&id).exists(),
        "the purge was expected to fail"
    );
    Ok(())
}

#[test]
fn evicting_an_entry_keeps_the_directory_access_of_a_job_restoring_it() -> Result<()> {
    let scratch = Scratch::new()?;
    let scope = scratch.0.join("scope");
    let aside = scratch.0.join("aside");
    fs::create_dir_all(scope.join("entry"))?;
    fs::create_dir(&aside)?;
    fs::write(scope.join("entry/data.tar"), b"x")?;
    fs::set_permissions(scope.join("entry"), fs::Permissions::from_mode(0o555))?;
    let scope_fd = open_path(&scope)?;
    let aside_fd = open_path(&aside)?;
    let held = openat(&scope_fd, "entry", directory_flags(), Mode::empty())?;
    remove_entry(scope_fd.as_fd(), OsStr::new("entry"), aside_fd.as_fd())?;
    assert!(!scope.join("entry").exists());
    assert_eq!(fstat(&held)?.st_mode & 0o777, 0o755);
    Ok(())
}
