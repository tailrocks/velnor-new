// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use velnor_ci::tools_local::{Inputs, Outcome, Verb, message, run};

use crate::tools_heal::{leftovers, save_as};
use crate::tools_verify::{Job, job};

fn aged(path: &Path, hours: u64) {
    File::open(path)
        .and_then(|dir| dir.set_modified(SystemTime::now() - Duration::from_hours(hours)))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn node(name: &str) -> (Job, PathBuf) {
    let job = job(name);
    let store = job.root.join("store");
    fs::create_dir_all(store.join("tools")).unwrap_or_else(|e| unreachable!("{e}"));
    (job, store)
}

fn saved_quietly(store: &Path, job: &Job, key: &str) {
    let saved = save_as(store, &job.home, key, false);
    assert!(
        matches!(&saved, Outcome::Saved { kept, .. } if kept.is_empty()),
        "{saved:?}"
    );
}

#[test]
fn jobs_holding_old_entries_open_cannot_grow_the_store_past_twice_the_kept_keys() {
    let (job, store) = node("tools-prune-held");
    let mut held = Vec::new();
    for (hours, key) in [(5, "k1"), (4, "k2"), (3, "k3"), (2, "k4")] {
        saved_quietly(&store, &job, key);
        let entry = store.join("tools").join(key);
        aged(&entry, hours);
        let open = File::open(&entry).unwrap_or_else(|e| unreachable!("{e}"));
        open.lock_shared().unwrap_or_else(|e| unreachable!("{e}"));
        held.push(open);
    }
    assert_eq!(leftovers(&store), ["k1", "k2", "k3", "k4"]);
    saved_quietly(&store, &job, "k5");
    assert_eq!(leftovers(&store), ["k2", "k3", "k4", "k5"]);
}

#[test]
fn an_old_entry_whose_name_no_key_can_have_is_still_pruned() {
    let (job, store) = node("tools-prune-long-name");
    let odd = store.join("tools").join("n".repeat(250));
    fs::create_dir_all(odd.join("payload")).unwrap_or_else(|e| unreachable!("{e}"));
    aged(&odd, 3);
    saved_quietly(&store, &job, "k1");
    saved_quietly(&store, &job, "k2");
    assert_eq!(leftovers(&store), ["k1", "k2"]);
}

#[test]
fn a_job_holding_an_entry_locked_cannot_turn_a_trusted_restore_into_a_miss() {
    let (job, store) = node("tools-prune-locked");
    saved_quietly(&store, &job, "k1");
    let locked = File::open(store.join("tools/k1")).unwrap_or_else(|e| unreachable!("{e}"));
    locked.lock().unwrap_or_else(|e| unreachable!("{e}"));
    let fresh = job.root.join("fresh");
    fs::create_dir_all(&fresh).unwrap_or_else(|e| unreachable!("{e}"));
    let shared = store.display().to_string();
    let restored = run(&Inputs {
        verb: Verb::Restore,
        key: "k1",
        event_name: "push",
        git_ref: "refs/heads/main",
        default_branch: Some("main"),
        shared_dir: Some(&shared),
        seed_dir: Some(&shared),
        home: &fresh,
        replace: false,
    });
    assert!(restored.hit(), "{restored:?}");
    assert!(
        message(&restored, "node-a").starts_with("::warning::"),
        "{restored:?}"
    );
    let rustc = fresh.join(".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/rustc");
    assert_eq!(
        fs::read_to_string(rustc).unwrap_or_default(),
        "rustc binary"
    );
}
