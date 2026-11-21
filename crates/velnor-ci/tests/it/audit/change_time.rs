// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::inventory::build_stamping;
use super::{audit, own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";
const INPUT: &str = "ties/stamped.txt";
const SIBLING: &str = "ties/sibling.txt";

const OLD: u64 = 1_000_000_000;

fn shifted(time: SystemTime, seconds: u64) -> SystemTime {
    time - Duration::from_secs(seconds)
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn declaring(path: &str, stamp: &[&str]) -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, INPUT, "");
    unit(&repo, SIBLING, "");
    unit(&repo, OUTPUT, &format!("cargo:rerun-if-changed={path}\n"));
    repo.set_mtime(INPUT, at(OLD));
    repo.set_mtime(SIBLING, at(OLD));
    build_stamping(&repo, OUTPUT, None, stamp);
    repo
}

fn built_repository() -> (Repo, SystemTime) {
    let repo = declaring("../../ties/stamped.txt", &[]);
    let built = repo.mtime(OUTPUT);
    (repo, built)
}

fn changed_at(repo: &Repo) -> SystemTime {
    let metadata = fs::metadata(repo.root().join(INPUT)).unwrap_or_else(|e| unreachable!("{e}"));
    let seconds = u64::try_from(metadata.ctime()).unwrap_or_else(|e| unreachable!("{e}"));
    let nanos = u32::try_from(metadata.ctime_nsec()).unwrap_or_else(|e| unreachable!("{e}"));
    UNIX_EPOCH + Duration::new(seconds, nanos)
}

fn chmod_until_changed_after(repo: &Repo, built: SystemTime) {
    let path = repo.root().join(INPUT);
    let deadline = Instant::now() + Duration::from_secs(30);
    for mode in [0o640, 0o644].into_iter().cycle() {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .unwrap_or_else(|e| unreachable!("{e}"));
        if changed_at(repo) > built {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the change time never passed the build time"
        );
    }
    assert_eq!(repo.mtime(INPUT), at(OLD));
}

fn assert_stale(repo: &Repo) {
    let refused = audit(repo, &["--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

fn assert_fresh(repo: &Repo, lock: &str) {
    let written = audit(repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        lock
    );
}

#[test]
fn an_input_untouched_since_the_build_is_fresh() {
    let (repo, _) = built_repository();
    assert_fresh(&repo, "ties/stamped.txt\tlib\n");
}

#[test]
fn an_input_whose_change_time_moves_after_the_build_is_stale() {
    let (repo, built) = built_repository();
    chmod_until_changed_after(&repo, built);
    assert_stale(&repo);
}

const STAMP: [&str; 2] = ["-t", "206501010000"];

fn tied_file(input: u64) -> Repo {
    let repo = declaring("../../ties/stamped.txt", &STAMP);
    repo.set_mtime(INPUT, shifted(repo.mtime(OUTPUT), input));
    repo
}

fn tied_directory(entry: u64) -> Repo {
    let repo = declaring("../../ties", &STAMP);
    repo.set_mtime(INPUT, shifted(repo.mtime(OUTPUT), entry));
    repo
}

#[test]
fn a_file_modified_in_the_same_instant_as_the_output_is_stale() {
    assert_stale(&tied_file(0));
}

#[test]
fn a_file_modified_just_before_the_output_is_fresh() {
    assert_fresh(&tied_file(1), "ties/stamped.txt\tlib\n");
}

#[test]
fn a_directory_entry_modified_in_the_same_instant_as_the_output_is_stale() {
    assert_stale(&tied_directory(0));
}

#[test]
fn a_directory_whose_entries_all_predate_the_output_is_fresh() {
    assert_fresh(&tied_directory(1), "ties\tlib\n");
}
