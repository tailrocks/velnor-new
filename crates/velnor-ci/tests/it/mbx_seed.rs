// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_ci::mbx_seed::{Inputs, Outcome, env_lines, message, select};

use crate::io::Repo;

struct Host {
    _repo: Repo,
    root: PathBuf,
}

impl Drop for Host {
    fn drop(&mut self) {
        for seed in ["seed", "linked", "followed", "empty"] {
            fs::set_permissions(self.root.join(seed), fs::Permissions::from_mode(0o755)).ok();
        }
    }
}

fn read_only(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn host(name: &str) -> Host {
    let repo = Repo::empty(name);
    let root = fs::canonicalize(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(root.join("temp")).unwrap_or_else(|e| unreachable!("{e}"));
    Host { _repo: repo, root }
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap_or(path)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, text).unwrap_or_else(|e| unreachable!("{e}"));
}

fn warm_seed(host: &Host) -> PathBuf {
    let seed = host.root.join("seed");
    for (path, text) in [
        ("actions/cas/ab/blob1", "one"),
        ("actions/cas/cd/blob2", "two!"),
        ("actions/executable-identities/id", "x"),
        ("actions/task-manifests/m/1", "manifest"),
        ("actions/action-results/ef/r1", "result"),
        ("actions/sessions/s1", "not copied"),
        ("actions/checkouts/c1", "not copied"),
        ("notice", "not copied"),
    ] {
        put(&seed.join(path), text);
    }
    read_only(&seed);
    seed
}

fn run(host: &Host, event: &str, git_ref: &str, seed: Option<&Path>) -> Outcome {
    run_within(host, event, git_ref, seed, Duration::from_secs(60))
}

fn run_within(
    host: &Host,
    event: &str,
    git_ref: &str,
    seed: Option<&Path>,
    budget: Duration,
) -> Outcome {
    let seed = seed.map(|path| path.display().to_string());
    let temp = host.root.join("temp");
    select(&Inputs {
        event_name: event,
        git_ref,
        default_branch: Some("main"),
        seed_dir: seed.as_deref(),
        runner_temp: &temp,
        owner: fs::metadata(&temp).map_or(0, |m| m.uid()),
        threads: 4,
        budget,
    })
}

fn tree(root: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .map(|rd| rd.flatten().collect())
            .unwrap_or_default();
        entries.sort_by_key(fs::DirEntry::path);
        for entry in entries {
            let path = entry.path();
            let rel = path.strip_prefix(base).unwrap_or(&path).display();
            let meta = fs::symlink_metadata(&path).unwrap_or_else(|e| unreachable!("{e}"));
            if meta.is_dir() {
                out.push(format!("{rel}/ {:o}", meta.mode() & 0o7777));
                walk(base, &path, out);
            } else {
                out.push(format!("{rel} {}", meta.len()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn a_pull_request_copies_only_the_action_store_subset_into_the_private_store() {
    let host = host("mbx-seed-copy");
    let seed = warm_seed(&host);
    let outcome = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    let store = host.root.join("temp/velnor/mbx");
    let Outcome::Seeded { copied, .. } = &outcome else {
        unreachable!("{outcome:?}")
    };
    assert_eq!(
        (copied.files, copied.bytes, copied.present, copied.failed),
        (5, 22, 0, 0)
    );
    assert!(!copied.expired);
    assert_eq!(
        env_lines(&outcome).expect("env lines"),
        format!("MBX_CACHE_DIR={}\n", store.display())
    );
    assert_eq!(
        fs::read_to_string(store.join("actions/cas/cd/blob2"))
            .ok()
            .as_deref(),
        Some("two!")
    );
    for gone in ["actions/sessions", "actions/checkouts", "notice"] {
        assert!(!store.join(gone).exists(), "{gone}");
    }
    assert!(
        message(&outcome).contains("5 files, 22 bytes"),
        "{}",
        message(&outcome)
    );
    assert!(fs::read_dir(store.join("actions/cas/ab")).is_ok_and(|d| d.count() == 1));
}

#[test]
fn a_seeded_file_keeps_the_modification_time_and_mode_of_its_source() {
    let host = host("mbx-seed-mtime");
    let seed = warm_seed(&host);
    let source = seed.join("actions/cas/cd/blob2");
    let old = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    fs::File::options()
        .write(true)
        .open(&source)
        .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(old)))
        .and_then(|()| fs::set_permissions(&source, fs::Permissions::from_mode(0o640)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    let copied = host.root.join("temp/velnor/mbx/actions/cas/cd/blob2");
    let metadata = fs::metadata(&copied).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(metadata.modified().ok(), Some(old));
    assert_eq!(metadata.mode() & 0o777, 0o640);
}

#[test]
fn a_second_run_skips_what_is_already_present() {
    let host = host("mbx-seed-again");
    let seed = warm_seed(&host);
    run(
        &host,
        "merge_group",
        "refs/heads/gh-readonly-queue/main/pr-1-a",
        Some(&seed),
    );
    put(&seed.join("actions/cas/ab/blob3"), "three");
    let again = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    let Outcome::Seeded { copied, .. } = again else {
        unreachable!("{again:?}")
    };
    assert_eq!((copied.files, copied.present), (1, 5));
}

#[test]
fn an_absent_seed_changes_nothing() {
    let host = host("mbx-seed-absent");
    let outcome = run(&host, "pull_request", "refs/pull/1/merge", None);
    assert_eq!(outcome, Outcome::NotOffered);
    assert_eq!(env_lines(&outcome).expect("env lines"), "");
    assert!(!host.root.join("temp/velnor").exists());
    let empty = host.root.join("empty");
    fs::create_dir_all(&empty).unwrap_or_else(|e| unreachable!("{e}"));
    read_only(&empty);
    let outcome = run(&host, "pull_request", "refs/pull/1/merge", Some(&empty));
    assert_eq!(outcome, Outcome::Empty);
    assert_eq!(env_lines(&outcome).expect("env lines"), "");
    assert!(!host.root.join("temp/velnor").exists());
}

#[test]
fn a_trusted_event_is_never_seeded() {
    let host = host("mbx-seed-trusted");
    let seed = warm_seed(&host);
    for event in ["push", "schedule"] {
        let outcome = run(&host, event, "refs/heads/main", Some(&seed));
        assert_eq!(outcome, Outcome::Trusted);
        assert_eq!(env_lines(&outcome).expect("env lines"), "");
    }
    assert!(!host.root.join("temp/velnor").exists());
}

#[test]
fn a_symlinked_or_unsafe_seed_is_refused() {
    let host = host("mbx-seed-refused");
    let seed = warm_seed(&host);
    let link = host.root.join("link");
    symlink(&seed, &link).unwrap_or_else(|e| unreachable!("{e}"));
    let open = host.root.join("open");
    fs::create_dir_all(&open).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&open, fs::Permissions::from_mode(0o777))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(host.root.join("file"), "").unwrap_or_else(|e| unreachable!("{e}"));
    let cases: [(PathBuf, &str); 6] = [
        (link, "is a symlink"),
        (host.root.join("link/actions"), "path contains a symlink"),
        (PathBuf::from("relative"), "not absolute"),
        (host.root.join("missing"), "does not exist"),
        (host.root.join("file"), "not a directory"),
        (open, "world-writable"),
    ];
    for (dir, reason) in cases {
        let outcome = run(&host, "pull_request", "refs/pull/1/merge", Some(&dir));
        assert_eq!(outcome, Outcome::Rejected(reason), "{}", dir.display());
        assert_eq!(env_lines(&outcome).expect("env lines"), "");
        assert!(message(&outcome).starts_with("::warning::VELNOR_MBX_SEED_DIR ignored"));
    }
    let wrong = select(&Inputs {
        event_name: "pull_request",
        git_ref: "refs/pull/1/merge",
        default_branch: Some("main"),
        seed_dir: seed.to_str(),
        runner_temp: &host.root.join("temp"),
        owner: 4_000_000,
        threads: 2,
        budget: Duration::from_secs(60),
    });
    let root_owned = fs::metadata(&seed).is_ok_and(|m| m.uid() == 0);
    if !root_owned {
        assert_eq!(wrong, Outcome::Rejected("owned by another user"));
    }
    assert!(!host.root.join("temp/velnor").exists());
}

#[test]
fn symlinks_inside_the_seed_are_never_followed_and_the_seed_is_never_written() {
    let host = host("mbx-seed-readonly");
    let seed = warm_seed(&host);
    let secret = host.root.join("secret");
    put(&secret, "outside");
    symlink(&secret, seed.join("actions/cas/ab/evil")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(
        host.root.join("elsewhere"),
        seed.join("actions/cas/dirlink"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&seed, fs::Permissions::from_mode(0o555))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let before = tree(&seed);
    let outcome = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    assert!(matches!(outcome, Outcome::Seeded { .. }), "{outcome:?}");
    assert_eq!(tree(&seed), before);
    let store = host.root.join("temp/velnor/mbx/actions/cas");
    assert!(!store.join("ab/evil").exists());
    assert!(!store.join("dirlink").exists());
    fs::set_permissions(&seed, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_private_store_that_is_a_symlink_is_refused() {
    let host = host("mbx-seed-store-link");
    let seed = warm_seed(&host);
    let victim = host.root.join("victim");
    fs::create_dir_all(&victim).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(host.root.join("temp/velnor")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&victim, host.root.join("temp/velnor/mbx")).unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    assert_eq!(
        outcome,
        Outcome::Rejected("private store path is not a directory")
    );
    assert_eq!(tree(&victim), Vec::<String>::new());
}

#[test]
fn a_seed_the_job_can_write_is_refused() {
    let host = host("mbx-seed-writable");
    let seed = warm_seed(&host);
    fs::set_permissions(&seed, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outcome = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    assert_eq!(outcome, Outcome::Rejected("writable by this job"));
    assert_eq!(env_lines(&outcome).expect("env lines"), "");
    assert!(!host.root.join("temp/velnor").exists());
}

#[test]
fn a_symlinked_subset_root_is_never_followed() {
    let host = host("mbx-seed-subset-link");
    let outside = host.root.join("outside");
    put(&outside.join("zz/secret"), "outside");
    let linked = host.root.join("linked");
    put(&linked.join("actions/task-manifests/m/1"), "manifest");
    symlink(&outside, linked.join("actions/cas")).unwrap_or_else(|e| unreachable!("{e}"));
    read_only(&linked);
    let outcome = run(&host, "pull_request", "refs/pull/1/merge", Some(&linked));
    let Outcome::Seeded { copied, .. } = &outcome else {
        unreachable!("{outcome:?}")
    };
    assert_eq!((copied.files, copied.failed), (1, 1));
    assert!(!host.root.join("temp/velnor/mbx/actions/cas").exists());
    assert!(message(&outcome).contains("::warning::1 seed entries failed to copy"));

    let followed = host.root.join("followed");
    fs::create_dir_all(&followed).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(linked.join("actions"), followed.join("actions"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    read_only(&followed);
    assert_eq!(
        run(&host, "pull_request", "refs/pull/1/merge", Some(&followed)),
        Outcome::Rejected("actions is a symlink")
    );
}

#[test]
fn a_copy_that_fails_is_reported_not_counted_as_success() {
    let host = host("mbx-seed-failed");
    let seed = warm_seed(&host);
    put(
        &host.root.join("temp/velnor/mbx/actions/cas/ab"),
        "in the way",
    );
    let outcome = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    let Outcome::Seeded { copied, .. } = &outcome else {
        unreachable!("{outcome:?}")
    };
    assert_eq!((copied.files, copied.failed), (4, 1));
    let text = message(&outcome);
    assert!(
        text.contains("::warning::1 seed entries failed to copy"),
        "{text}"
    );
}

#[test]
fn seeding_stops_at_the_startup_budget_and_says_so() {
    let host = host("mbx-seed-budget");
    let seed = warm_seed(&host);
    let outcome = run_within(
        &host,
        "pull_request",
        "refs/pull/1/merge",
        Some(&seed),
        Duration::ZERO,
    );
    let Outcome::Seeded { copied, .. } = &outcome else {
        unreachable!("{outcome:?}")
    };
    assert_eq!((copied.files, copied.failed), (0, 0));
    assert!(copied.expired);
    assert!(message(&outcome).contains("::warning::MBX seeding stopped at the startup budget"));
    let within = run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    let Outcome::Seeded { copied, .. } = &within else {
        unreachable!("{within:?}")
    };
    assert_eq!((copied.files, copied.expired), (5, false));
}

#[test]
fn a_seeded_file_never_keeps_a_setuid_or_setgid_bit_from_its_source() {
    let host = host("mbx-seed-setuid");
    let seed = warm_seed(&host);
    let source = seed.join("actions/cas/cd/blob2");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o6755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    run(&host, "pull_request", "refs/pull/1/merge", Some(&seed));
    let copied = host.root.join("temp/velnor/mbx/actions/cas/cd/blob2");
    let mode = fs::metadata(&copied)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .mode();
    assert_eq!(mode & 0o6000, 0, "copied mode {mode:o} keeps setuid/setgid");
}
