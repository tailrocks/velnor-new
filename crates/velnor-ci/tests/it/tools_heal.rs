// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use velnor_ci::tools_discard;
use velnor_ci::tools_local::{Inputs, Outcome, Verb, run};
use velnor_ci::tools_verify::Verb as VerifyVerb;
use velnor_ci::tools_verify_step::run as verify_step;

use crate::io::Repo;
use crate::tools_verify::{Job, context, job, recorded, rustc};

const KEY: &str = "velnor-tools-Linux-X64-mise2026.11.10-abc123";

fn save(store: &Path, home: &Path, replace: bool) -> Outcome {
    let store = store.display().to_string();
    run(&Inputs {
        verb: Verb::Save,
        key: KEY,
        event_name: "push",
        git_ref: "refs/heads/main",
        default_branch: Some("main"),
        shared_dir: Some(&store),
        seed_dir: Some(&store),
        home,
        owner: fs::metadata(home).map_or(0, |metadata| metadata.uid()),
        replace,
    })
}

fn poisoned_store(name: &str) -> (Repo, Job, PathBuf) {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let store = root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let job = job(name);
    let saved = save(&store, &job.home, false);
    assert!(matches!(saved, Outcome::Saved { .. }), "{saved:?}");
    let entry = store.join("tools").join(KEY);
    let planted = entry.join(".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/rustc");
    fs::write(&planted, "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    (repo, job, store)
}

fn stored_rustc(store: &Path) -> String {
    fs::read_to_string(
        store
            .join("tools")
            .join(KEY)
            .join(".rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/rustc"),
    )
    .unwrap_or_default()
}

#[test]
fn a_poisoned_entry_is_kept_until_a_discard_asks_for_its_replacement() {
    let (_repo, job, store) = poisoned_store("tools-heal-kept");
    let home = &job.home;
    assert_eq!(save(&store, home, false), Outcome::AlreadySaved);
    assert_eq!(stored_rustc(&store), "evil binary!");
}

#[test]
fn a_discard_lets_the_verified_tree_replace_the_poisoned_entry() {
    let (_repo, job, store) = poisoned_store("tools-heal-replaced");
    let home = &job.home;
    let healed = save(&store, home, true);
    assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
    assert_eq!(stored_rustc(&store), "rustc binary");
    assert_eq!(save(&store, home, false), Outcome::AlreadySaved);
    let leftovers: Vec<_> = fs::read_dir(store.join("tools"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, [KEY], "no retired or staging entry stays");
}

#[test]
fn a_replacement_that_cannot_remove_the_old_entry_fails_instead_of_healing() {
    let (_repo, job, store) = poisoned_store("tools-heal-undeletable");
    let entry = store.join("tools").join(KEY);
    fs::remove_dir_all(&entry).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&entry, "not a tree").unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = save(&store, &job.home, true);
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
}

#[test]
fn a_replacement_with_no_entry_to_replace_is_an_ordinary_save() {
    let repo = Repo::empty("tools-heal-no-entry");
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let store = root.join("store");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    let job = job("tools-heal-no-entry-home");
    let outcome = save(&store, &job.home, true);
    assert!(matches!(outcome, Outcome::Saved { .. }), "{outcome:?}");
}

#[test]
fn a_mismatching_restore_marks_the_job_and_a_clean_one_does_not() {
    let job = job("tools-heal-mark");
    let anchor = recorded(&job);
    verify_step(VerifyVerb::Check, &context(&job, &anchor, "push"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!tools_discard::pending(&job.root));
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    verify_step(VerifyVerb::Check, &context(&job, &anchor, "push"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(tools_discard::pending(&job.root));
    tools_discard::clear(&job.root);
    assert!(!tools_discard::pending(&job.root));
}
