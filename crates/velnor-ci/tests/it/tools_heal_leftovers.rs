// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use velnor_ci::tools_local::{Outcome, message};

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
fn a_stale_legacy_random_leftover_is_kept_while_another_save_runs() {
    let (_repo, job, store) = poisoned_store("tools-heal-legacy-held");
    let tools = store.join("tools");
    let lock = tools.join(format!(".lock-{OTHER}"));
    let leftover = tools.join(".staging-retired-0123456789abcdef");
    fs::create_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir(&leftover).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(leftover.join("rustc"), "in use").unwrap_or_else(|e| unreachable!("{e}"));
    aged(&leftover, Duration::from_hours(2));
    let healed = save(&store, &job.home, true);
    assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
    assert!(
        leftover.join("rustc").is_file(),
        "a legacy retired tree was removed while another save runs"
    );
    fs::remove_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    let healed = save(&store, &job.home, true);
    assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
    assert_eq!(leftovers(&store), [KEY], "the legacy leftover stays");
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
fn a_prune_candidate_whose_save_is_running_is_not_removed() {
    let job = job("tools-heal-prune-held");
    let store = job.root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let first = save_as(&store, &job.home, "velnor-tools-first", false);
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
    let lock = store.join("tools/.lock-velnor-tools-first");
    fs::create_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    let third = save_as(&store, &job.home, "velnor-tools-third", false);
    assert!(matches!(third, Outcome::Saved { .. }), "{third:?}");
    assert!(
        store.join("tools/velnor-tools-first").is_dir(),
        "the prune removed an entry whose save is still running"
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

fn two_aged_entries(job: &Job, store: &Path) -> PathBuf {
    let first = save_as(store, &job.home, "velnor-tools-first", false);
    assert!(matches!(first, Outcome::Saved { .. }), "{first:?}");
    aged(
        &store.join("tools/velnor-tools-first"),
        Duration::from_hours(3),
    );
    let second = save_as(store, &job.home, "velnor-tools-second", false);
    assert!(matches!(second, Outcome::Saved { .. }), "{second:?}");
    aged(
        &store.join("tools/velnor-tools-second"),
        Duration::from_hours(2),
    );
    store.join("tools/velnor-tools-first")
}

#[test]
fn a_prune_candidate_whose_lock_is_released_is_removed_by_the_next_save() {
    let job = job("tools-heal-prune-released");
    let store = job.root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let first = two_aged_entries(&job, &store);
    let lock = store.join("tools/.lock-velnor-tools-first");
    fs::create_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    let third = save_as(&store, &job.home, "velnor-tools-third", false);
    assert!(matches!(third, Outcome::Saved { .. }), "{third:?}");
    assert!(first.is_dir(), "a locked prune candidate was removed");
    fs::remove_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    let fourth = save_as(&store, &job.home, "velnor-tools-fourth", false);
    assert!(matches!(fourth, Outcome::Saved { .. }), "{fourth:?}");
    assert!(!first.exists(), "a released prune candidate was kept");
}

#[test]
fn a_prune_candidate_replaced_while_its_lock_was_held_is_kept() {
    let job = job("tools-heal-prune-replaced");
    let store = job.root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let first = two_aged_entries(&job, &store);
    let modified = fs::metadata(&first)
        .and_then(|metadata| metadata.modified())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let lock = store.join("tools/.lock-velnor-tools-first");
    fs::create_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    let parked = store.join("parked");
    let replacement = first.clone();
    let replace = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        fs::rename(&replacement, &parked)?;
        fs::create_dir(&replacement)?;
        fs::File::open(&replacement)?.set_modified(modified)?;
        fs::remove_dir(&lock)
    });
    let third = save_as(&store, &job.home, "velnor-tools-third", false);
    assert!(matches!(third, Outcome::Saved { .. }), "{third:?}");
    replace
        .join()
        .unwrap_or_else(|_| unreachable!("the replacing thread panicked"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(first.is_dir(), "the prune removed a replacement entry");
}

#[test]
fn a_save_does_not_wait_for_a_prune_candidate_whose_lock_is_held() {
    let job = job("tools-heal-prune-no-wait");
    let store = job.root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let first = two_aged_entries(&job, &store);
    let lock = store.join("tools/.lock-velnor-tools-first");
    fs::create_dir(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    let started = Instant::now();
    let third = save_as(&store, &job.home, "velnor-tools-third", false);
    let elapsed = started.elapsed();
    assert!(matches!(third, Outcome::Saved { .. }), "{third:?}");
    assert!(
        elapsed < Duration::from_secs(5),
        "the save waited {elapsed:?} for a prune candidate whose lock is held"
    );
    assert!(first.is_dir(), "the locked prune candidate was removed");
}

#[test]
fn a_prune_candidate_that_cannot_be_locked_is_kept_and_reported() {
    let job = job("tools-heal-prune-unlockable");
    let store = job.root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let name = "k".repeat(250);
    let candidate = store.join("tools").join(&name);
    fs::create_dir_all(&candidate).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(candidate.join("rustc"), "old binary").unwrap_or_else(|e| unreachable!("{e}"));
    aged(&candidate, Duration::from_hours(3));
    let second = save_as(&store, &job.home, "velnor-tools-second", false);
    assert!(matches!(second, Outcome::Saved { .. }), "{second:?}");
    aged(
        &store.join("tools/velnor-tools-second"),
        Duration::from_hours(2),
    );
    let third = save_as(&store, &job.home, "velnor-tools-third", false);
    assert!(matches!(third, Outcome::Saved { .. }), "{third:?}");
    assert!(
        candidate.join("rustc").is_file(),
        "the unlockable entry was removed"
    );
    let text = message(&third, "node");
    assert!(
        text.contains("::warning::") && text.contains(&name),
        "the unlockable entry was not reported: {text}"
    );
}

#[test]
fn a_stale_symlink_entry_is_pruned_without_touching_its_target() {
    let job = job("tools-heal-prune-symlink");
    let store = job.root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let target = job.root.join("outside");
    fs::create_dir_all(&target).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(target.join("keep"), "outside").unwrap_or_else(|e| unreachable!("{e}"));
    let link = store.join("tools/velnor-tools-first");
    fs::create_dir_all(store.join("tools")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&target, &link).unwrap_or_else(|e| unreachable!("{e}"));
    let touched = Command::new("touch")
        .args(["-h", "-t", "201001010000"])
        .arg(&link)
        .status();
    assert!(touched.is_ok_and(|status| status.success()));
    let second = save_as(&store, &job.home, "velnor-tools-second", false);
    assert!(matches!(second, Outcome::Saved { .. }), "{second:?}");
    let third = save_as(&store, &job.home, "velnor-tools-third", false);
    assert!(matches!(third, Outcome::Saved { .. }), "{third:?}");
    assert!(
        fs::symlink_metadata(&link).is_err(),
        "the stale symlink stays"
    );
    assert!(
        target.join("keep").is_file(),
        "the symlink target was removed"
    );
}
