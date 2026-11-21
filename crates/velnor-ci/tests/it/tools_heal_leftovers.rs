// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, SystemTime};

use velnor_ci::tools_local::Outcome;

use crate::io::Repo;
use crate::tools_heal::{leftovers, poisoned_store, save};

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
fn a_failed_copy_leaves_no_staging_tree_behind_even_when_it_holds_a_read_only_directory() {
    let repo = Repo::empty("tools-heal-failed-copy");
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let store = root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let home = root.join("home");
    let mise = home.join(".local/share/mise/installs");
    fs::create_dir_all(&mise).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(mise.join("cosign"), "cosign binary").unwrap_or_else(|e| unreachable!("{e}"));
    let read_only = home.join(".rustup/toolchains/read-only");
    fs::create_dir_all(&read_only).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(read_only.join("rustc"), "rustc binary").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let unreadable = home.join(".rustup/unreadable");
    fs::write(&unreadable, "no access").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = save(&store, &home, false);
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert_eq!(
        leftovers(&store),
        Vec::<String>::new(),
        "a staging tree stays"
    );
}
