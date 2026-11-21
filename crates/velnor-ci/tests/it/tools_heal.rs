// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

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

#[test]
fn an_unwritable_discard_marker_is_reported_and_the_discard_still_happens() {
    let job = job("tools-heal-marker-blocked");
    let anchor = recorded(&job);
    fs::write(rustc(&job), "evil binary!").unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir(job.root.join("velnor-tools-discarded")).unwrap_or_else(|e| unreachable!("{e}"));
    let out = verify_step(VerifyVerb::Check, &context(&job, &anchor, "push"))
        .unwrap_or_else(|e| unreachable!("the verify step aborted: {e}"));
    assert!(
        out.contains("::notice::the restored tools were discarded"),
        "{out}"
    );
    assert!(
        out.contains("::warning::cannot record the discard"),
        "{out}"
    );
    assert!(!job.home.join(".rustup").exists());
}

#[cfg(target_os = "macos")]
struct Immutable(PathBuf);

#[cfg(target_os = "macos")]
impl Immutable {
    fn set(path: &Path) -> Self {
        let status = Command::new("chflags")
            .arg("uchg")
            .arg(path)
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success());
        let locked = Self(path.to_path_buf());
        let probe = path.with_file_name("probe-retire");
        assert!(
            fs::rename(path, &probe).is_err(),
            "the entry must be unmovable for this test to mean anything"
        );
        locked
    }
}

#[cfg(target_os = "macos")]
impl Drop for Immutable {
    fn drop(&mut self) {
        Command::new("chflags")
            .arg("nouchg")
            .arg(&self.0)
            .status()
            .ok();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_replacement_that_cannot_retire_the_poisoned_entry_fails_and_keeps_it() {
    let (_repo, job, store) = poisoned_store("tools-heal-unretired");
    let _locked = Immutable::set(&store.join("tools").join(KEY));
    let outcome = save(&store, &job.home, true);
    assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
    assert!(velnor_ci::tools_local::finish(Verb::Save, &outcome, "node").is_err());
    assert_eq!(stored_rustc(&store), "evil binary!");
    let leftovers: Vec<_> = fs::read_dir(store.join("tools"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, [KEY], "no staging or retired entry stays");
}

fn save_in_a_working_directory(name: &str, runner_temp: Option<&str>, marker_dir: &str) {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let store = root.join("store");
    let home = root.join("home");
    let event = root.join("event.json");
    fs::create_dir_all(&store).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(home.join(".local/share/mise")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(home.join(".rustup/toolchains/stable"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(home.join(".rustup/toolchains/stable/rustc"), "rustc")
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&event, r#"{"repository":{"default_branch":"main"}}"#)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let marker = root.join(marker_dir).join("velnor-tools-discarded");
    fs::create_dir_all(marker.parent().unwrap_or(&root)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&marker, "").unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["tools-local", "save", "--key", KEY])
        .current_dir(&root)
        .env_remove("RUNNER_TEMP")
        .env("HOME", &home)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_EVENT_PATH", &event)
        .env("VELNOR_MBX_SHARED_DIR", &store)
        .env("VELNOR_MBX_SEED_DIR", &store);
    if let Some(temp) = runner_temp {
        command.env("RUNNER_TEMP", temp);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("tools saved to the node store"), "{stdout}");
    assert!(
        marker.is_file(),
        "a successful save deleted the working directory's marker"
    );
}

#[test]
fn an_unset_runner_temp_neither_reads_nor_deletes_a_marker_in_the_working_directory() {
    save_in_a_working_directory("tools-heal-cli-unset", None, ".");
}

#[test]
fn an_empty_runner_temp_neither_reads_nor_deletes_a_marker_in_the_working_directory() {
    save_in_a_working_directory("tools-heal-cli-empty", Some(""), ".");
}

#[test]
fn a_relative_runner_temp_is_treated_like_an_unset_one() {
    save_in_a_working_directory(
        "tools-heal-cli-relative",
        Some("relative-temp"),
        "relative-temp",
    );
}

fn verify_in_a_working_directory(name: &str, verb: &str, runner_temp: &str) {
    let job = job(name);
    let output = job.root.join("github-output");
    let run = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_clear()
        .args(["tools-verify", verb])
        .current_dir(&job.root)
        .env("HOME", &job.home)
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REF", "refs/heads/main")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!run.status.success(), "{run:?}");
    assert!(stderr.contains("RUNNER_TEMP"), "{stderr}");
    assert!(stderr.contains("absolute"), "{stderr}");
    let entries: Vec<_> = fs::read_dir(&job.root)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|entry| entry.starts_with("velnor-") || entry == "relative-temp")
        .collect();
    assert!(
        entries.is_empty(),
        "wrote into the working directory: {entries:?}"
    );
}

#[test]
fn tools_verify_refuses_an_empty_runner_temp_and_writes_nothing_in_the_working_directory() {
    for verb in ["read", "check", "record"] {
        verify_in_a_working_directory(&format!("tools-verify-empty-{verb}"), verb, "");
    }
}

#[test]
fn tools_verify_refuses_a_relative_runner_temp_and_writes_nothing_in_the_working_directory() {
    for verb in ["read", "check", "record"] {
        verify_in_a_working_directory(
            &format!("tools-verify-relative-{verb}"),
            verb,
            "relative-temp",
        );
    }
}
