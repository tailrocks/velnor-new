// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use rustix::fs::{Mode, OFlags};

use super::{Bounds, Removal, ascend, bounds_on, contents, make_owner_accessible};

type Check = Result<(), Box<dyn std::error::Error>>;

fn open(path: &Path) -> Result<OwnedFd, rustix::io::Errno> {
    rustix::fs::open(path, OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty())
}

fn clear(top: &Path, bounds: Bounds) -> Result<(), Removal> {
    contents(&open(top)?, bounds)
}

fn device(path: &Path) -> Result<i128, Box<dyn std::error::Error>> {
    Ok(super::device_of(&rustix::fs::fstat(open(path)?)?))
}

fn nest(root: &Path, levels: usize) -> Check {
    let mut path = root.to_path_buf();
    for _ in 0..levels {
        path.push("d");
    }
    std::fs::create_dir_all(path)?;
    Ok(())
}

#[test]
fn a_tree_with_files_siblings_and_nesting_is_emptied() -> Check {
    let root = tempfile::tempdir()?;
    let top = root.path().join("top");
    for directory in ["a/b/c", "a/d", "e"] {
        std::fs::create_dir_all(top.join(directory))?;
    }
    for file in ["f", "a/g", "a/b/h", "a/b/c/i", "e/j"] {
        std::fs::write(top.join(file), "")?;
    }
    clear(&top, bounds_on(device(&top)?)).map_err(|error| format!("{error:?}"))?;
    assert_eq!(std::fs::read_dir(&top)?.count(), 0);
    Ok(())
}

#[test]
fn more_entries_than_the_bound_stop_the_removal_and_name_it() -> Check {
    let root = tempfile::tempdir()?;
    let top = root.path().join("top");
    std::fs::create_dir(&top)?;
    for index in 0..5 {
        std::fs::write(top.join(format!("f{index}")), "")?;
    }
    let bounds = Bounds {
        depth: 10,
        entries: 3,
        device: device(&top)?,
    };
    assert_eq!(clear(&top, bounds), Err(Removal::TooMany));
    clear(&top, bounds_on(device(&top)?)).map_err(|error| format!("{error:?}"))?;
    assert_eq!(std::fs::read_dir(&top)?.count(), 0);
    Ok(())
}

#[test]
fn a_tree_deeper_than_the_bound_stops_the_removal_and_names_it() -> Check {
    let root = tempfile::tempdir()?;
    let top = root.path().join("top");
    nest(&top, 6)?;
    let bounds = Bounds {
        depth: 3,
        entries: 100,
        device: device(&top)?,
    };
    assert_eq!(clear(&top, bounds), Err(Removal::TooDeep));
    contents(&open(&top)?, bounds_on(device(&top)?)).map_err(|error| format!("{error:?}"))?;
    assert_eq!(std::fs::read_dir(&top)?.count(), 0);
    Ok(())
}

#[test]
fn ascending_from_a_directory_that_moved_is_refused() -> Check {
    let root = tempfile::tempdir()?;
    std::fs::create_dir_all(root.path().join("one/child"))?;
    std::fs::create_dir(root.path().join("two"))?;
    let one = rustix::fs::fstat(open(&root.path().join("one"))?)?;
    let two = rustix::fs::fstat(open(&root.path().join("two"))?)?;
    let child = open(&root.path().join("one/child"))?;
    assert!(ascend(child.as_fd(), &one).is_ok());
    assert!(matches!(ascend(child.as_fd(), &two), Err(Removal::Moved)));
    Ok(())
}

#[test]
fn a_symlink_in_place_of_a_locked_directory_never_changes_its_target() -> Check {
    let root = tempfile::tempdir()?;
    let target = root.path().join("target");
    std::fs::create_dir(&target)?;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000))?;
    std::os::unix::fs::symlink(&target, root.path().join("link"))?;
    let _outcome = make_owner_accessible(&open(root.path())?, "link");
    let mode = std::fs::metadata(&target)?.permissions().mode() & 0o777;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700))?;
    assert_eq!(mode, 0o000, "the target of a symlink was chmodded");
    Ok(())
}

#[test]
fn a_locked_directory_becomes_accessible_to_its_owner() -> Check {
    let root = tempfile::tempdir()?;
    let locked = root.path().join("locked");
    std::fs::create_dir(&locked)?;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))?;
    make_owner_accessible(&open(root.path())?, "locked")?;
    assert_eq!(
        std::fs::metadata(&locked)?.permissions().mode() & 0o700,
        0o700
    );
    Ok(())
}

#[test]
fn a_tree_on_another_device_than_the_store_is_refused_untouched() -> Check {
    let root = tempfile::tempdir()?;
    let top = root.path().join("top");
    std::fs::create_dir(&top)?;
    std::fs::write(top.join("kept"), "")?;
    let elsewhere = bounds_on(device(&top)?.wrapping_add(1));
    assert_eq!(clear(&top, elsewhere), Err(Removal::Crossed));
    assert!(top.join("kept").exists());
    Ok(())
}

#[test]
fn a_directory_on_another_device_inside_the_tree_is_never_entered() -> Check {
    let root = tempfile::tempdir()?;
    let top = root.path().join("top");
    std::fs::create_dir_all(top.join("mounted"))?;
    std::fs::write(top.join("mounted/kept"), "")?;
    let handle = open(&top)?;
    let here = super::device_of(&rustix::fs::fstat(&handle)?);
    let outcome = super::open_directory(&handle, "mounted", here.wrapping_add(1));
    assert_eq!(outcome.err(), Some(Removal::Crossed));
    assert!(top.join("mounted/kept").exists());
    Ok(())
}

#[test]
fn a_locked_mount_point_of_another_device_is_never_chmodded() -> Check {
    let root = tempfile::tempdir()?;
    let top = root.path().join("top");
    std::fs::create_dir(&top)?;
    let handle = open(&top)?;
    let here = super::device_of(&rustix::fs::fstat(&handle)?);
    let locked = top.join("locked");
    std::fs::create_dir(&locked)?;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))?;
    let outcome = super::open_directory(&handle, "locked", here.wrapping_add(1));
    let mode = std::fs::metadata(&locked)?.permissions().mode() & 0o777;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700))?;
    assert_eq!(outcome.err(), Some(Removal::Crossed));
    assert_eq!(
        mode, 0o000,
        "the mount point of another device was chmodded"
    );
    Ok(())
}
