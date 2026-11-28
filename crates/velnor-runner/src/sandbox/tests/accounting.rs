// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rustix::fs::{CWD, Mode, OFlags, mkdirat, openat};
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

use super::super::Layout;
use super::super::dirs;
use super::super::disk::{self, Bounds, Usage};
use super::super::watch::start;

const MIB: usize = 1 << 20;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("velnor-sandbox-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    assert!(std::fs::create_dir_all(&path).is_ok());
    path
}

fn used(directories: &[&Path]) -> Usage {
    disk::used(directories, &AtomicBool::new(false))
}

fn limit_descriptors(wanted: u64) {
    let limit = getrlimit(Resource::Nofile);
    assert!(
        setrlimit(
            Resource::Nofile,
            Rlimit {
                current: Some(wanted),
                maximum: limit.maximum,
            },
        )
        .is_ok()
    );
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

fn allocated(paths: &[PathBuf]) -> u64 {
    paths
        .iter()
        .map(|path| std::fs::symlink_metadata(path).map_or(0, |metadata| metadata.blocks() * 512))
        .sum()
}

#[test]
fn disk_use_counts_the_allocated_bytes_of_every_inode_exactly_once() {
    let root = scratch("disk");
    assert!(std::fs::create_dir_all(root.join("a/b")).is_ok());
    assert!(std::fs::write(root.join("a/b/data"), vec![1_u8; MIB]).is_ok());
    assert!(std::fs::hard_link(root.join("a/b/data"), root.join("a/link")).is_ok());
    assert!(std::fs::write(root.join("small"), b"x").is_ok());
    let inodes: Vec<PathBuf> = ["", "a", "a/b", "a/b/data", "small"]
        .iter()
        .map(|part| root.join(part))
        .collect();
    let expected = allocated(&inodes);
    assert_eq!(used(&[root.as_path()]).bytes, expected);
    assert_eq!(
        used(&[root.as_path(), root.as_path()]).bytes,
        expected,
        "a directory named twice was counted twice"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_walk_counts_down_to_its_depth_cap_and_nothing_below_it() {
    let seen = scratch("seen");
    let unseen = scratch("unseen");
    nest(&seen, 40, 4 * MIB);
    nest(&unseen, 60, 4 * MIB);
    let stop = AtomicBool::new(false);
    let counted = disk::used_within(
        &[seen.as_path()],
        Bounds {
            depth: 50,
            entries: 1000,
        },
        &stop,
    );
    assert!(
        counted.bytes >= (4 * MIB) as u64 && !counted.foreign,
        "{counted:?}"
    );
    let below = disk::used_within(
        &[unseen.as_path()],
        Bounds {
            depth: 50,
            entries: 1000,
        },
        &stop,
    );
    assert!(
        below.bytes < MIB as u64 && !below.foreign,
        "the walk went past its depth cap: {below:?}"
    );
    assert!(disk::overage(below, 64).is_none());
    for root in [seen, unseen] {
        let _ = dirs::remove(&root);
    }
}

#[test]
fn a_tree_nested_200_levels_is_removed_with_a_handful_of_descriptors() {
    limit_descriptors(48);
    let root = scratch("deep");
    nest(&root, 200, 4 * MIB);
    assert!(dirs::remove(&root).is_ok(), "the deep tree was not removed");
    assert!(!root.exists());
}

#[test]
fn the_removal_finishes_a_tree_an_interrupted_run_left_flattened() {
    let root = scratch("rerun");
    for name in [".velnor-flat-0", ".velnor-flat-1"] {
        assert!(std::fs::create_dir_all(root.join(name).join("inner")).is_ok());
        assert!(std::fs::write(root.join(name).join("inner/file"), b"x").is_ok());
    }
    assert!(std::fs::create_dir_all(root.join("a/b/c")).is_ok());
    assert!(std::fs::write(root.join("a/b/c/file"), b"x").is_ok());
    assert!(
        dirs::remove(&root).is_ok(),
        "the leftovers wedged the removal"
    );
    assert!(!root.exists());
    assert!(dirs::remove(&root).is_ok(), "removing twice must converge");
}

#[test]
fn the_walk_stops_at_its_entry_cap_and_keeps_what_it_counted() {
    let root = scratch("entries");
    for index in 0..300 {
        assert!(std::fs::write(root.join(format!("f{index}")), vec![1_u8; 4096]).is_ok());
    }
    let stop = AtomicBool::new(false);
    let capped = disk::used_within(
        &[root.as_path()],
        Bounds {
            depth: 50,
            entries: 100,
        },
        &stop,
    );
    let all = disk::used_within(
        &[root.as_path()],
        Bounds {
            depth: 50,
            entries: 10_000,
        },
        &stop,
    );
    assert!(
        capped.bytes > 0 && capped.bytes < all.bytes / 2,
        "{capped:?} against {all:?}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dropping_a_started_walk_cancels_it() {
    let root = scratch("cancel");
    let layout = Layout {
        work_dir: root.clone(),
        stage_dir: root.clone(),
        ..super::layout()
    };
    let walk = start(&layout).expect("a walk starts");
    let stop = std::sync::Arc::clone(&walk.stop);
    assert!(!stop.load(std::sync::atomic::Ordering::Relaxed));
    drop(walk);
    assert!(stop.load(std::sync::atomic::Ordering::Relaxed));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_directory_wider_than_one_batch_is_removed_completely() {
    let root = scratch("wide");
    for index in 0..1300 {
        assert!(std::fs::create_dir_all(root.join(format!("d{index}/inner"))).is_ok());
        assert!(std::fs::write(root.join(format!("d{index}/inner/file")), b"x").is_ok());
        assert!(std::fs::write(root.join(format!("f{index}")), b"x").is_ok());
    }
    assert!(dirs::remove(&root).is_ok(), "the wide tree was not removed");
    assert!(!root.exists());
}

#[test]
fn disk_overage_names_the_use_and_the_limit() {
    let used = |bytes| Usage {
        bytes,
        foreign: false,
    };
    assert_eq!(disk::overage(used(64 << 30), 64), None);
    assert_eq!(
        disk::overage(used((65 << 30) + 5), 64).as_deref(),
        Some("velnor: job used 65 GiB of disk; the limit is 64 GiB")
    );
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
