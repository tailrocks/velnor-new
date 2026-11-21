// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, SystemTime};

use velnor_ci::tools_local::Outcome;

use crate::io::Repo;
use crate::tools_heal::{KEY, leftovers, poisoned_store, save, save_as};
use crate::tools_verify::{Job, job};

const OTHER: &str = "velnor-tools-Linux-X64-mise2026.11.10-def456";
const READ_ONLY: &str = ".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin";

fn aged(path: &Path, age: Duration) {
    fs::File::open(path)
        .and_then(|file| file.set_modified(SystemTime::now() - age))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn read_only_home(job: &Job) -> impl FnOnce() + use<> {
    let bin = job.home.join(READ_ONLY);
    mode(&bin, 0o555);
    move || mode(&bin, 0o755)
}

#[test]
fn a_stale_read_only_leftover_with_a_random_name_is_swept_by_the_next_save() {
    let (_repo, job, store) = poisoned_store("tools-heal-random-leftover");
    let leftover = store
        .join("tools")
        .join(".staging-retired-0123456789abcdef");
    fs::create_dir_all(&leftover).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(leftover.join("rustc"), "old binary").unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::open(&leftover)
        .and_then(|dir| dir.set_modified(SystemTime::now() - Duration::from_hours(2)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&leftover, fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let healed = save(&store, &job.home, true);
    assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
    assert!(
        fs::symlink_metadata(&leftover).is_err(),
        "the stale leftover stays"
    );
}

#[test]
fn pruning_removes_an_old_entry_holding_a_read_only_directory() {
    let job = job("tools-heal-prune-read-only");
    let store = job.root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let release = read_only_home(&job);
    let first = save_as(&store, &job.home, "velnor-tools-first", false);
    release();
    assert!(matches!(first, Outcome::Saved { .. }), "{first:?}");
    aged(
        &store.join("tools/velnor-tools-first"),
        Duration::from_hours(3),
    );
    let second = save_as(&store, &job.home, "velnor-tools-second", false);
    assert!(matches!(second, Outcome::Saved { .. }), "{second:?}");
    aged(
        &store.join("tools/velnor-tools-second"),
        Duration::from_hours(2),
    );
    let third = save_as(&store, &job.home, "velnor-tools-third", false);
    assert!(matches!(third, Outcome::Saved { .. }), "{third:?}");
    assert_eq!(
        leftovers(&store),
        ["velnor-tools-second", "velnor-tools-third"],
        "the oldest entry stays"
    );
}

#[test]
fn a_stale_leftover_of_a_key_whose_save_is_running_is_not_swept() {
    let (_repo, job, store) = poisoned_store("tools-heal-live-retired");
    let tools = store.join("tools");
    let lock = tools.join(format!(".lock-{OTHER}"));
    let retired = tools.join(format!(".staging-retired-{OTHER}"));
    let staging = tools.join(format!(".staging-{OTHER}-0123456789abcdef"));
    fs::create_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    for tree in [&retired, &staging] {
        fs::create_dir(tree).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(tree.join("rustc"), "in use").unwrap_or_else(|e| unreachable!("{e}"));
        aged(tree, Duration::from_hours(2));
    }
    let healed = save(&store, &job.home, true);
    assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
    assert!(
        retired.join("rustc").is_file(),
        "the running save lost its retired tree"
    );
    assert!(
        staging.join("rustc").is_file(),
        "the running save lost its staging tree"
    );
    fs::remove_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    let healed = save(&store, &job.home, true);
    assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
    assert_eq!(
        leftovers(&store),
        [KEY],
        "the finished save's leftovers stay"
    );
}

#[test]
fn a_stale_regular_file_at_the_retired_path_still_fails_the_replacement() {
    let (_repo, job, store) = poisoned_store("tools-heal-stale-retired-file");
    let tools = store.join("tools");
    let blocker = tools.join(format!(".staging-retired-{KEY}"));
    fs::write(&blocker, "").unwrap_or_else(|e| unreachable!("{e}"));
    aged(&blocker, Duration::from_hours(2));
    let outcome = save(&store, &job.home, true);
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert!(blocker.is_file(), "the blocker was removed");
    let stored = tools.join(KEY).join(READ_ONLY).join("rustc");
    assert_eq!(
        fs::read_to_string(stored).unwrap_or_default(),
        "evil binary!"
    );
}

#[test]
fn a_blocked_retirement_leaves_no_read_only_staging_tree_behind() {
    let (_repo, job, store) = poisoned_store("tools-heal-blocked-read-only");
    let blocker = format!(".staging-retired-{KEY}");
    fs::write(store.join("tools").join(&blocker), "").unwrap_or_else(|e| unreachable!("{e}"));
    let release = read_only_home(&job);
    let outcome = save(&store, &job.home, true);
    release();
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    let mut expected = vec![KEY.to_owned(), blocker];
    expected.sort();
    assert_eq!(leftovers(&store), expected, "a staging tree stays");
}

#[test]
fn a_failed_publication_leaves_no_read_only_staging_tree_behind() {
    let (_repo, job, store) = poisoned_store("tools-heal-unpublished-read-only");
    let tools = store.join("tools");
    fs::remove_file(tools.join(KEY).join(".complete")).unwrap_or_else(|e| unreachable!("{e}"));
    let blocker = format!(".staging-retired-{KEY}");
    fs::write(tools.join(&blocker), "").unwrap_or_else(|e| unreachable!("{e}"));
    let release = read_only_home(&job);
    let outcome = save(&store, &job.home, false);
    release();
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    let mut expected = vec![KEY.to_owned(), blocker];
    expected.sort();
    assert_eq!(leftovers(&store), expected, "a staging tree stays");
}

#[test]
fn a_failed_copy_leaves_no_staging_tree_behind_even_when_it_holds_a_read_only_directory() {
    let repo = Repo::empty("tools-heal-failed-copy");
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let store = root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let home = root.join("home");
    let read_only = home.join(".local/share/mise/installs/read-only");
    fs::create_dir_all(&read_only).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(read_only.join("cosign"), "cosign binary").unwrap_or_else(|e| unreachable!("{e}"));
    mode(&read_only, 0o555);
    fs::write(home.join(".rustup"), "not a directory").unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = save(&store, &home, false);
    mode(&read_only, 0o755);
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(
        leftovers(&store),
        Vec::<String>::new(),
        "a staging tree stays"
    );
}
