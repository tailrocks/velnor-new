// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

use rustix::fs::{Mode, OFlags};

use super::{BOUNDS, Bounds, Removal, ascend, contents};

type Check = Result<(), Box<dyn std::error::Error>>;

fn open(path: &Path) -> Result<OwnedFd, rustix::io::Errno> {
    rustix::fs::open(path, OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty())
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
fn a_tree_with_files_siblings_and_nesting_is_emptied_whole() -> Check {
    let root = tempfile::tempdir()?;
    let top = root.path().join("top");
    for directory in ["a/b/c", "a/d", "e"] {
        std::fs::create_dir_all(top.join(directory))?;
    }
    for file in ["f", "a/g", "a/b/h", "a/b/c/i", "e/j"] {
        std::fs::write(top.join(file), "")?;
    }
    contents(&open(&top)?, BOUNDS).map_err(|error| format!("{error:?}"))?;
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
    };
    assert_eq!(contents(&open(&top)?, bounds), Err(Removal::TooMany));
    assert!(std::fs::read_dir(&top)?.count() > 0);
    contents(&open(&top)?, BOUNDS).map_err(|error| format!("{error:?}"))?;
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
    };
    assert_eq!(contents(&open(&top)?, bounds), Err(Removal::TooDeep));
    contents(&open(&top)?, BOUNDS).map_err(|error| format!("{error:?}"))?;
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
