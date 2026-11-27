// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tar::{EntryType, Header};

use super::*;
use crate::native::Umask;
use crate::native::cache::archive::{Cancelled, pack};

const HUGE: u64 = 4 << 30;

struct Cleanup(PathBuf);

fn unlock(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir()) {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
        for item in fs::read_dir(path).into_iter().flatten().flatten() {
            unlock(&item.path());
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        unlock(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_restore_stops_inside_a_large_file_once_the_job_is_cancelled() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let root = dir.join("dest");
    fs::create_dir_all(&root)?;
    let mut header = Header::new_gnu();
    header.set_path("0/big")?;
    header.set_entry_type(EntryType::Regular);
    header.set_mode(0o644);
    header.set_size(HUGE);
    header.set_cksum();
    let archive = dir.join("in.tar");
    let mut file = File::create(&archive)?;
    file.write_all(header.as_bytes())?;
    file.set_len(512 + HUGE + 1024)?;
    let cancel = JobCancel::new();
    let started = Instant::now();
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(20));
            cancel.cancel();
        });
        unpack(&archive, &[root], Umask::read(), Some(&cancel))
    });
    let elapsed = started.elapsed();
    fs::remove_dir_all(&dir)?;
    assert!(result.is_err_and(|error| error.is::<Cancelled>()));
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
    Ok(())
}

fn write_tar(archive: &Path, members: &[(&str, EntryType, Option<&str>)]) -> Result<()> {
    let mut builder = tar::Builder::new(File::create(archive)?);
    for (path, kind, link) in members {
        let mut header = Header::new_gnu();
        header.set_entry_type(*kind);
        header.set_mode(if *kind == EntryType::Directory {
            0o755
        } else {
            0o644
        });
        let body: &[u8] = if *kind == EntryType::Regular {
            b"data"
        } else {
            b""
        };
        header.set_size(body.len() as u64);
        match link {
            Some(target) => builder.append_link(&mut header, path, target)?,
            None => builder.append_data(&mut header, path, body)?,
        }
    }
    builder.finish()?;
    Ok(())
}

fn unpacked(members: &[(&str, EntryType, Option<&str>)]) -> Result<(Result<u64>, PathBuf)> {
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let root = dir.join("dest");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("outside"), "outside")?;
    let archive = dir.join("in.tar");
    write_tar(&archive, members)?;
    let result = unpack(&archive, &[root], Umask::read(), None);
    Ok((result, dir))
}

fn refused_with(members: &[(&str, EntryType, Option<&str>)], message: &str) -> Result<()> {
    let (result, dir) = unpacked(members)?;
    let _cleanup = Cleanup(dir.clone());
    let linked = fs::symlink_metadata(dir.join("dest/b")).is_ok();
    let error = result.err().map(|error| format!("{error:#}"));
    assert!(
        error.as_deref().is_some_and(|text| text.contains(message)),
        "{error:?}"
    );
    assert!(!linked, "the refused hard link was created");
    Ok(())
}

#[test]
fn a_hard_link_reaching_outside_the_declared_paths_is_refused() -> Result<()> {
    refused_with(
        &[
            ("0/", EntryType::Directory, None),
            ("0/a", EntryType::Regular, None),
            ("0/b", EntryType::Link, Some("0/../outside")),
        ],
        "escapes the declared paths",
    )
}

#[test]
fn a_hard_link_to_a_symlink_member_is_refused() -> Result<()> {
    refused_with(
        &[
            ("0/", EntryType::Directory, None),
            ("0/s", EntryType::Symlink, Some("elsewhere")),
            ("0/b", EntryType::Link, Some("0/s")),
        ],
        "does not name a regular file",
    )
}

#[test]
fn overlapping_roots_with_a_hard_linked_file_round_trip() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    let data = dir.join("data");
    fs::create_dir_all(data.join("sub"))?;
    fs::write(data.join("sub/a"), "linked")?;
    fs::hard_link(data.join("sub/a"), data.join("sub/b"))?;
    let roots = [data.clone(), data.join("sub")];
    let archive = dir.join("in.tar");
    pack(&roots, File::create(&archive)?, None)?;
    fs::remove_dir_all(&data)?;
    unpack(&archive, &roots, Umask::read(), None)?;
    let names = (
        fs::read_to_string(data.join("sub/a")).ok(),
        fs::read_to_string(data.join("sub/b")).ok(),
    );
    let same = fs::metadata(data.join("sub/a"))?.ino() == fs::metadata(data.join("sub/b"))?.ino();
    assert_eq!(
        names,
        (Some("linked".to_owned()), Some("linked".to_owned()))
    );
    assert!(same);
    Ok(())
}

#[test]
fn finalizing_the_directories_stops_once_the_job_is_cancelled() -> Result<()> {
    const DIRECTORIES: usize = 4000;
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    let root = dir.join("dest");
    fs::create_dir_all(&root)?;
    let mut restore = Restore::new(std::slice::from_ref(&root), Umask::read());
    restore.open(0)?;
    for index in 0..DIRECTORIES {
        let name = format!("d{index}");
        fs::create_dir(root.join(&name))?;
        restore.pending.push(Pending {
            index: 0,
            path: vec![name.into()],
            bits: 0o750,
            stamp: None,
        });
    }
    let done = |index: usize| {
        fs::metadata(root.join(format!("d{index}"))).is_ok_and(|meta| meta.mode() & 0o777 == 0o750)
    };
    let cancel = JobCancel::new();
    let finished = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::SeqCst) && !done(DIRECTORIES - 1) {
                std::thread::yield_now();
            }
            cancel.cancel();
        });
        let result = restore.finish(Some(&cancel));
        finished.store(true, Ordering::SeqCst);
        result
    });
    assert!(result.is_err_and(|error| error.is::<Cancelled>()));
    assert!((0..DIRECTORIES).filter(|index| done(*index)).count() < DIRECTORIES);
    Ok(())
}

#[test]
fn a_nested_root_is_restored_below_its_outer_root_after_the_outer_replaced_a_link() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    let source = dir.join("source/data");
    fs::create_dir_all(source.join("sub"))?;
    fs::write(source.join("sub/f"), "inner")?;
    fs::write(source.join("top"), "outer")?;
    let archive = dir.join("in.tar");
    pack(
        &[source.clone(), source.join("sub")],
        File::create(&archive)?,
        None,
    )?;
    let workspace = dir.join("workspace");
    let outside = dir.join("outside");
    fs::create_dir_all(&workspace)?;
    fs::create_dir(&outside)?;
    std::os::unix::fs::symlink(&outside, workspace.join("data"))?;
    let data = workspace.join("data");
    unpack(
        &archive,
        &[data.clone(), data.join("sub")],
        Umask::read(),
        None,
    )?;
    let restored = fs::read_to_string(data.join("sub/f")).ok();
    let leaked = fs::read_dir(&outside)?.count();
    assert!(fs::symlink_metadata(&data)?.is_dir());
    assert_eq!(restored.as_deref(), Some("inner"));
    assert_eq!(leaked, 0);
    Ok(())
}

#[test]
fn a_nested_root_under_an_outer_root_restored_as_a_file_is_refused() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    let workspace = dir.join("workspace");
    fs::create_dir_all(&workspace)?;
    let archive = dir.join("in.tar");
    write_tar(
        &archive,
        &[
            ("0", EntryType::Regular, None),
            ("1/", EntryType::Directory, None),
        ],
    )?;
    let data = workspace.join("data");
    let result = unpack(
        &archive,
        &[data.clone(), data.join("sub")],
        Umask::read(),
        None,
    );
    let error = result.err().map(|error| format!("{error:#}"));
    assert!(
        error
            .as_deref()
            .is_some_and(|text| text.contains("was restored as a link or file")),
        "{error:?}"
    );
    assert!(fs::symlink_metadata(&data)?.is_file());
    Ok(())
}

#[test]
fn a_root_replaced_by_a_link_is_never_written_through_by_a_declared_path_aliasing_it() -> Result<()>
{
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    let workspace = dir.join("workspace");
    let victim = dir.join("victim");
    fs::create_dir_all(workspace.join("data"))?;
    fs::create_dir(&victim)?;
    std::os::unix::fs::symlink("data", workspace.join("hop"))?;
    std::os::unix::fs::symlink("hop", workspace.join("h2"))?;
    let archive = dir.join("in.tar");
    write_tar(
        &archive,
        &[
            ("0", EntryType::Symlink, Some("../victim")),
            ("1/", EntryType::Directory, None),
        ],
    )?;
    let result = unpack(
        &archive,
        &[workspace.join("hop"), workspace.join("h2/z")],
        Umask::read(),
        None,
    );
    let error = result.err().map(|error| format!("{error:#}"));
    assert!(
        error
            .as_deref()
            .is_some_and(|text| text.contains("was restored as a link or file")),
        "{error:?}"
    );
    assert_eq!(fs::read_dir(&victim)?.count(), 0);
    Ok(())
}

#[test]
fn directories_created_implicitly_take_the_mode_gnu_tar_gives_them_under_the_umask() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    let workspace = dir.join("workspace");
    fs::create_dir_all(&workspace)?;
    let archive = dir.join("in.tar");
    write_tar(
        &archive,
        &[
            ("0/", EntryType::Directory, None),
            ("0/x/f", EntryType::Regular, None),
        ],
    )?;
    let root = workspace.join("a/b");
    let previous = rustix::process::umask(rustix::fs::Mode::from_bits_truncate(0o002));
    let result = unpack(&archive, std::slice::from_ref(&root), Umask::read(), None);
    rustix::process::umask(previous);
    result?;
    let mode = |path: PathBuf| fs::metadata(path).map(|meta| meta.mode() & 0o7777);
    assert_eq!(mode(workspace.join("a"))?, 0o775);
    assert_eq!(mode(root.join("x"))?, 0o775);
    Ok(())
}

#[test]
fn declared_paths_that_each_resolve_below_the_other_are_refused() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    let workspace = dir.join("workspace");
    let outer = workspace.join("d/y");
    let middle = outer.join("z");
    fs::create_dir_all(&middle)?;
    std::os::unix::fs::symlink("../../..", middle.join("lnk"))?;
    let inner = middle.join("lnk/d");
    let archive = dir.join("in.tar");
    write_tar(&archive, &[("0/", EntryType::Directory, None)])?;
    let result = unpack(&archive, &[outer, middle, inner], Umask::read(), None);
    let error = result.err().map(|error| format!("{error:#}"));
    assert!(
        error
            .as_deref()
            .is_some_and(|text| text.contains("each resolve below the other")),
        "{error:?}"
    );
    Ok(())
}

#[test]
fn four_thousand_declared_paths_restore_within_a_bounded_time() -> Result<()> {
    const PATHS: usize = 4000;
    const BOUND: Duration = Duration::from_secs(60);
    let dir = std::env::temp_dir().join(format!("velnor-restore-{}", uuid::Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    let workspace = dir.join("workspace");
    fs::create_dir_all(&workspace)?;
    let names: Vec<String> = (0..PATHS).map(|index| format!("{index}/")).collect();
    let members: Vec<(&str, EntryType, Option<&str>)> = names
        .iter()
        .map(|root| (root.as_str(), EntryType::Directory, None))
        .collect();
    let archive = dir.join("in.tar");
    write_tar(&archive, &members)?;
    let roots: Vec<PathBuf> = (0..PATHS)
        .map(|index| workspace.join(format!("p{index}")))
        .collect();
    let cancel = JobCancel::new();
    let finished = AtomicBool::new(false);
    let started = Instant::now();
    let restored = std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::SeqCst) && started.elapsed() < BOUND {
                std::thread::sleep(Duration::from_millis(10));
            }
            cancel.cancel();
        });
        let restored = unpack(&archive, &roots, Umask::read(), Some(&cancel));
        finished.store(true, Ordering::SeqCst);
        restored
    });
    assert_eq!(restored?, PATHS as u64);
    assert!(workspace.join(format!("p{}", PATHS - 1)).is_dir());
    Ok(())
}
