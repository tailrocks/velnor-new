// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs::{self, File, FileTimes};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::super::inventory::build_stamping;
use super::super::{audit, own_units, unit, workspace_below_the_root};
use crate::io::Repo;

mod restore;

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

fn restore(repo: &Repo) {
    let restored = super::super::velnor_ci()
        .current_dir(repo.root())
        .args(["restored", "--target-dir", "rust/target"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(restored.status.success(), "{restored:?}");
}

fn saved(repo: &Repo) {
    repo.git(&["add", "--all"]);
    repo.commit();
    let saved = super::super::velnor_ci()
        .current_dir(repo.root())
        .args(["inputs", "save", "--target-dir", "rust/target"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(saved.status.success(), "{saved:?}");
}

fn restore_inputs(repo: &Repo, operation: &[&OsStr]) {
    let restored = super::super::velnor_ci()
        .current_dir(repo.root())
        .env("GITHUB_JOB", "job")
        .args(operation)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(restored.status.success(), "{restored:?}");
}

const INPUTS_RESTORE: [&str; 4] = ["inputs", "restore", "--target-dir", "rust/target"];

fn restored_input() -> (Repo, SystemTime) {
    let (repo, built) = built_repository();
    saved(&repo);
    repo.set_mtime(INPUT, at(OLD + 1));
    restore_inputs(&repo, &INPUTS_RESTORE.map(OsStr::new));
    assert_eq!(repo.mtime(INPUT), at(OLD));
    (repo, built)
}

#[test]
fn an_input_whose_change_time_moved_before_a_restore_that_left_it_alone_is_stale() {
    let (repo, built) = built_repository();
    chmod_until_changed_after(&repo, built);
    restore(&repo);
    assert_stale(&repo);
}

#[test]
fn an_input_changed_after_the_restore_is_stale() {
    let (repo, built) = restored_input();
    chmod_until_changed_after(&repo, built);
    assert_stale(&repo);
}

#[test]
fn a_rewrite_with_its_modified_time_restored_after_the_restore_is_stale() {
    let (repo, built) = restored_input();
    repo.write_unstaged(INPUT, "");
    repo.set_mtime(INPUT, at(OLD));
    assert!(repo.mtime(INPUT) < built);
    assert_stale(&repo);
}

#[test]
fn a_changed_input_with_its_modified_time_put_back_before_the_restore_is_stale() {
    let (repo, built) = built_repository();
    saved(&repo);
    repo.write_unstaged(INPUT, "changed");
    repo.set_mtime(INPUT, at(OLD));
    restore_inputs(&repo, &INPUTS_RESTORE.map(OsStr::new));
    assert!(repo.mtime(INPUT) < built);
    assert_stale(&repo);
}

fn restored_by(operation: &[&OsStr]) {
    let (repo, _) = built_repository();
    saved(&repo);
    repo.set_mtime(INPUT, at(OLD + 1));
    restore_inputs(&repo, operation);
    assert_eq!(repo.mtime(INPUT), at(OLD));
    assert_fresh(&repo, "ties/stamped.txt\tlib\n");
}

#[test]
fn inputs_restored_by_inputs_restore_are_fresh() {
    restored_by(&INPUTS_RESTORE.map(OsStr::new));
}

#[test]
fn inputs_restored_by_inputs_restore_all_are_fresh() {
    let scopes = Repo::empty("scope-cache");
    let cache = scopes.root().join("scopes.json");
    restored_by(&[
        "inputs".as_ref(),
        "restore-all".as_ref(),
        "--root-target".as_ref(),
        ".".as_ref(),
        "rust/target".as_ref(),
        "--scope-cache".as_ref(),
        cache.as_os_str(),
    ]);
}

fn nested_directory() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, "staging/sub/value.txt", "other");
    unit(&repo, "ties/sub/value.txt", "built");
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../../ties\n");
    for path in ["staging/sub/value.txt", "ties/sub/value.txt"] {
        repo.set_mtime(path, at(OLD));
    }
    for directory in ["staging/sub", "staging", "ties/sub", "ties"] {
        File::open(repo.root().join(directory))
            .and_then(|file| file.set_times(FileTimes::new().set_modified(at(OLD))))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    settled_after(&repo, "ties");
    build_stamping(&repo, OUTPUT, None, &[]);
    repo
}

fn swap_in_the_staged_directory(repo: &Repo) {
    let root = repo.root();
    fs::rename(root.join("ties/sub"), root.join("staging/gone"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::rename(root.join("staging/sub"), root.join("ties/sub"))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_nested_directory_replaced_by_one_prepared_before_the_build_is_stale() {
    let repo = nested_directory();
    swap_in_the_staged_directory(&repo);
    assert_stale(&repo);
}

#[test]
fn a_nested_directory_replaced_after_the_restore_is_stale() {
    let repo = nested_directory();
    saved(&repo);
    restore_inputs(&repo, &INPUTS_RESTORE.map(OsStr::new));
    swap_in_the_staged_directory(&repo);
    assert_stale(&repo);
}

#[test]
fn directories_touched_during_the_restore_are_fresh() {
    let repo = nested_directory();
    saved(&repo);
    repo.write_unstaged("ties/sub/transient.txt", "");
    fs::remove_file(repo.root().join("ties/sub/transient.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    restore_inputs(&repo, &INPUTS_RESTORE.map(OsStr::new));
    assert_fresh(&repo, "ties\tlib\n");
}

#[test]
fn a_directory_touched_after_the_build_without_a_restore_is_stale() {
    let repo = nested_directory();
    repo.write_unstaged("ties/sub/transient.txt", "");
    fs::remove_file(repo.root().join("ties/sub/transient.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_stale(&repo);
}

fn settled_after(repo: &Repo, path: &str) {
    let ctime = |path: &str| {
        let metadata =
            fs::symlink_metadata(repo.root().join(path)).unwrap_or_else(|e| unreachable!("{e}"));
        (metadata.ctime(), metadata.ctime_nsec())
    };
    let created = ctime(path);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        repo.write_unstaged(".git/settle.probe", "");
        if ctime(".git/settle.probe") > created {
            break;
        }
        assert!(Instant::now() < deadline, "the clock never advanced");
    }
}

const GENERATED: &str = "rust/target/generated/link.rs";

fn link_generated(repo: &Repo) {
    let link = repo.root().join(GENERATED);
    fs::create_dir_all(link.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let _removed = fs::remove_file(&link);
    std::os::unix::fs::symlink(repo.root().join("shared/old.rs"), &link)
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn linked_source(retarget_after_the_build: bool) -> Repo {
    let repo = workspace_below_the_root();
    unit(&repo, "shared/old.rs", "");
    repo.set_mtime("shared/old.rs", at(OLD));
    settled_after(&repo, "shared/old.rs");
    if !retarget_after_the_build {
        link_generated(&repo);
        settled_after(&repo, "rust/target/generated/link.rs");
    }
    own_units(&repo, "rust/target/debug", &[]);
    unit(
        &repo,
        "rust/target/debug/deps/app-f000000000000001.d",
        "out.d: app/src/lib.rs target/generated/link.rs\n",
    );
    unit(&repo, OUTPUT, "");
    build_stamping(&repo, OUTPUT, None, &[]);
    if retarget_after_the_build {
        link_generated(&repo);
    }
    repo
}

#[test]
fn a_generated_link_retargeted_to_an_older_file_after_the_build_is_stale() {
    let repo = linked_source(true);
    assert_stale(&repo);
}

#[test]
fn a_generated_link_untouched_since_the_build_is_fresh() {
    let repo = linked_source(false);
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
}
