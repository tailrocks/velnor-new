// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rustix::fs::{CWD, Mode, OFlags, mkdirat, openat};
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

use super::super::dirs;
use super::super::disk::{self, Usage};

const MIB: usize = 1 << 20;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("velnor-sandbox-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    assert!(std::fs::create_dir_all(&path).is_ok());
    path
}

fn used(directories: &[&Path]) -> Usage {
    disk::used(
        directories,
        Path::new("/nonexistent"),
        &AtomicBool::new(false),
    )
}

fn open(parent: &OwnedFd, name: &str, flags: OFlags) -> OwnedFd {
    openat(
        parent,
        name,
        flags | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o700),
    )
    .expect("open a level of the tree")
}

fn nest(root: &Path, levels: usize, payload: usize) {
    let mut current = open(
        &openat(CWD, root, OFlags::RDONLY | OFlags::DIRECTORY, Mode::empty())
            .expect("open the root"),
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY,
    );
    for _ in 0..levels {
        mkdirat(&current, "d", Mode::from_raw_mode(0o700)).expect("create a level");
        current = open(&current, "d", OFlags::RDONLY | OFlags::DIRECTORY);
    }
    let file = open(&current, "payload", OFlags::WRONLY | OFlags::CREATE);
    assert!(
        std::fs::File::from(file)
            .write_all(&vec![1_u8; payload])
            .is_ok()
    );
}

#[test]
fn disk_use_counts_allocated_blocks_of_every_file_once() {
    let root = scratch("disk");
    assert!(std::fs::create_dir_all(root.join("a/b")).is_ok());
    assert!(std::fs::write(root.join("a/b/data"), vec![1_u8; MIB]).is_ok());
    assert!(std::fs::hard_link(root.join("a/b/data"), root.join("a/link")).is_ok());
    assert!(std::fs::write(root.join("small"), b"x").is_ok());
    let once = used(&[root.as_path()]);
    assert!(once.bytes >= MIB as u64 && !once.hidden, "{once:?}");
    assert!(
        once.bytes < (MIB + MIB / 4) as u64,
        "the hard link was counted twice: {once:?}"
    );
    let twice = used(&[root.as_path(), root.as_path()]);
    assert!(
        twice.bytes < (MIB + MIB / 4) as u64,
        "a directory named twice was counted twice: {twice:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_tree_nested_3000_levels_is_counted_and_removed_under_a_256_descriptor_limit() {
    let limit = getrlimit(Resource::Nofile);
    assert!(
        setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(256),
                maximum: limit.maximum,
            },
        )
        .is_ok()
    );
    let root = scratch("deep");
    nest(&root, 3000, 4 * MIB);
    let counted = used(&[root.as_path()]);
    assert!(
        counted.bytes >= (4 * MIB) as u64 && !counted.hidden,
        "the payload at the bottom was not counted: {counted:?}"
    );
    assert!(dirs::remove(&root).is_ok(), "the deep tree was not removed");
    assert!(!root.exists());
}

#[test]
fn a_root_that_cannot_be_walked_counts_as_hiding_files() {
    let root = scratch("unreadable");
    let file = root.join("not-a-directory");
    assert!(std::fs::write(&file, b"x").is_ok());
    let link = root.join("link");
    assert!(std::os::unix::fs::symlink(&root, &link).is_ok());
    for path in [&file, &link] {
        let usage = used(&[path.as_path()]);
        assert!(usage.hidden, "{path:?}");
        assert!(
            disk::overage(usage, 64)
                .is_some_and(|message| message.contains("hid files from its accounting")),
            "{path:?}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn disk_overage_names_the_use_and_the_limit() {
    let used = |bytes| Usage {
        bytes,
        hidden: false,
        foreign: false,
    };
    assert_eq!(disk::overage(used(64 << 30), 64), None);
    assert_eq!(
        disk::overage(used((65 << 30) + 5), 64).as_deref(),
        Some("velnor: job used 65 GiB of disk; the limit is 64 GiB")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_unlinked_file_held_by_two_descriptors_is_counted_once() {
    let root = scratch("held");
    let cgroup = root.join("cgroup");
    assert!(std::fs::create_dir_all(&cgroup).is_ok());
    assert!(
        std::fs::write(
            cgroup.join("cgroup.procs"),
            format!("{}\n", std::process::id())
        )
        .is_ok()
    );
    let file = root.join("hidden");
    let mut first = std::fs::File::create(&file).expect("create the file");
    assert!(first.write_all(&vec![1_u8; MIB]).is_ok());
    let second = std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .expect("open it again");
    assert!(std::fs::remove_file(&file).is_ok());
    let usage = disk::used(&[], &cgroup, &AtomicBool::new(false));
    drop((first, second));
    assert!(
        usage.bytes < (MIB + MIB / 2) as u64,
        "the unlinked file was counted once per descriptor: {usage:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_machine_layout_must_be_provisioned_root_owned_and_closed_to_others() {
    let root = scratch("provision");
    let missing = root.join("missing");
    let open_to_all = root.join("open");
    assert!(std::fs::create_dir(&open_to_all).is_ok());
    assert!(
        std::fs::set_permissions(
            &open_to_all,
            std::os::unix::fs::PermissionsExt::from_mode(0o777)
        )
        .is_ok()
    );
    for path in [&missing, &open_to_all, &root] {
        let refusal = dirs::provisioned(path).expect_err("an unprovisioned directory was accepted");
        assert!(
            format!("{refusal:#}").contains("provision the machine"),
            "{refusal:#}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}
