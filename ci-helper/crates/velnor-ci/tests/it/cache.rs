// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::fs;
use std::process::{Command, Output};

use velnor_ci::cache::{born_expired, stale_entries, trim};
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite, Repo};
use velnor_ci::time::date_text;

use crate::io::Repo as Checkout;

const DAY: i64 = 86_400;
const NOW: i64 = 1_794_873_600;
const COMPILED: &str =
    "   Compiling app v0.1.0 (/work/lab/app)\n    Finished `dev` profile in 3.20s\n";
const WARM: &str = "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.40s\n";

fn decide(repo: &Checkout, log: Option<&str>) -> (Output, String) {
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    if let Some(log) = log {
        fs::write(temp.join("velnor-run.log"), log).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let outputs = repo.root().join("github-output");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([
            "cache",
            "decide",
            "--target-dir",
            "target",
            "--archive",
            "nextest-archive.tar.zst",
        ])
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    (output, written)
}

fn seed(repo: &Checkout) {
    repo.write_unstaged("target/debug/incremental/app-1/work", "x");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    repo.write_unstaged("nextest-archive.tar.zst", "archive");
}

#[test]
fn a_run_that_compiled_a_crate_saves_after_dropping_what_must_not_be_cached() {
    let repo = Checkout::new("decide-compiled");
    seed(&repo);
    let (output, written) = decide(&repo, Some(COMPILED));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=true\n");
    assert!(stdout.contains("1 crates compiled"), "{stdout}");
    assert!(stdout.contains(" bytes"), "{stdout}");
    assert!(!repo.root().join("target/debug/incremental").exists());
    assert!(!repo.root().join("nextest-archive.tar.zst").exists());
    assert!(repo.root().join("target/debug/deps/libapp.rlib").is_file());
}

#[test]
fn a_warm_run_or_a_run_with_no_log_saves_nothing_and_removes_nothing() {
    for log in [Some(WARM), None] {
        let repo = Checkout::new("decide-warm");
        seed(&repo);
        let (output, written) = decide(&repo, log);
        assert!(output.status.success());
        assert_eq!(written, "save=false\n");
        assert!(repo.root().join("target/debug/incremental").is_dir());
        assert!(repo.root().join("nextest-archive.tar.zst").is_file());
        assert!(!repo.root().join("target/.velnor/born").exists());
    }
}

#[test]
fn a_cold_build_writes_the_born_date_and_a_later_save_carries_it_forward() {
    let repo = Checkout::new("decide-born");
    seed(&repo);
    decide(&repo, Some(COMPILED));
    let born = fs::read_to_string(repo.root().join("target/.velnor/born"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(born.len(), "2026-11-17\n".len());
    repo.write_unstaged("target/.velnor/born", "2026-01-02\n");
    decide(&repo, Some(COMPILED));
    let kept = fs::read_to_string(repo.root().join("target/.velnor/born"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(kept, "2026-01-02\n");
}

fn restored(repo: &Checkout, days: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.arg("restored").current_dir(repo.root());
    if let Some(days) = days {
        command.env("VELNOR_BASE_MAX_AGE_DAYS", days);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn aged(name: &str, born: &str) -> Checkout {
    let repo = Checkout::new(name);
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    repo.write_unstaged("target/.velnor/born", born);
    repo
}

#[test]
fn restored_removes_a_build_directory_born_more_than_the_limit_ago() {
    let old = date_text(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0))
            - 8 * DAY,
    );
    let repo = aged("expire-old", &format!("{old}\n"));
    let output = restored(&repo, Some("7"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("builds cold"), "{stdout}");
    assert!(!repo.root().join("target").exists());
    assert!(repo.root().join("src/lib.rs").is_file());
}

#[test]
fn restored_keeps_a_young_build_directory() {
    let today = date_text(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0)),
    );
    let young = aged("expire-young", &format!("{today}\n"));
    assert!(restored(&young, Some("7")).status.success());
    assert!(young.root().join("target/debug/deps/libapp.rlib").is_file());
}

#[test]
fn restored_keeps_an_old_build_directory_when_no_limit_is_set() {
    let unlimited = aged("expire-unlimited", "2020-01-01\n");
    for days in [None, Some("0")] {
        assert!(restored(&unlimited, days).status.success());
        assert!(
            unlimited
                .root()
                .join("target/debug/deps/libapp.rlib")
                .is_file()
        );
    }
}

#[test]
fn the_born_date_expires_after_exactly_the_limit_and_an_unreadable_one_expires_at_once() {
    let born = date_text(NOW);
    assert!(!born_expired(&born, NOW + 7 * DAY, 7));
    assert!(born_expired(&born, NOW + 8 * DAY, 7));
    assert!(born_expired("garbage", NOW, 7));
    assert!(born_expired("", NOW, 7));
}

#[test]
fn dates_are_written_as_the_utc_calendar_day() {
    assert_eq!(date_text(0), "1970-01-01");
    assert_eq!(date_text(951_782_400 + 3_600), "2000-02-29");
    assert_eq!(date_text(NOW - 1), "2026-11-16");
    assert_eq!(date_text(NOW), "2026-11-17");
}

struct FakeCaches {
    listing: String,
    asked: RefCell<Vec<String>>,
    deleted: RefCell<Vec<String>>,
}

impl GitHub for FakeCaches {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.asked.borrow_mut().push(path.to_owned());
        Ok(self.listing.clone())
    }
}

impl GitHubWrite for FakeCaches {
    fn upload(&self, _: &str, _: &[u8]) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn patch(&self, _: &str, _: &str) -> Result<String, CiError> {
        Err(CiError::new("unused"))
    }
    fn delete(&self, path: &str) -> Result<(), CiError> {
        self.deleted.borrow_mut().push(path.to_owned());
        Ok(())
    }
}

fn listing() -> String {
    let entry = |id: u64, key: &str, day: u32| {
        format!(r#"{{"id":{id},"key":"{key}","created_at":"2026-11-{day:02}T10:00:00Z"}}"#)
    };
    let family = "velnor-tgt-rust-lint-lab-Linux-X64-abcd-";
    let other = "velnor-tgt-rust-lint-lab-Linux-X64-9999-";
    let entries = [
        entry(1, &format!("{family}h1-1"), 1),
        entry(5, &format!("{family}h1-5"), 5),
        entry(3, &format!("{family}h1-3"), 3),
        entry(4, &format!("{family}h1-4"), 4),
        entry(2, &format!("{family}h1-2"), 2),
        entry(9, &format!("{other}h1-9"), 1),
        entry(10, "velnor-tgt-rust-test-lab-Linux-X64-abcd-h1-1", 1),
    ];
    format!(
        r#"{{"total_count":7,"actions_caches":[{}]}}"#,
        entries.join(",")
    )
}

#[test]
fn trim_keeps_the_newest_two_of_one_family_and_compat_and_touches_nothing_else() {
    let fake = FakeCaches {
        listing: listing(),
        asked: RefCell::default(),
        deleted: RefCell::default(),
    };
    let repo = Repo::parse("tailrocks/velnor-lab").unwrap_or_else(|e| unreachable!("{e}"));
    let report = trim(
        &fake,
        &repo,
        "refs/heads/main",
        "velnor-tgt-rust-lint-lab-Linux-X64-abcd-",
        2,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        *fake.deleted.borrow(),
        [
            "/repos/tailrocks/velnor-lab/actions/caches/3",
            "/repos/tailrocks/velnor-lab/actions/caches/2",
            "/repos/tailrocks/velnor-lab/actions/caches/1",
        ]
    );
    assert!(report.contains("deleted 3"), "{report}");
    let asked = fake.asked.borrow()[0].clone();
    assert!(asked.contains("ref=refs%2Fheads%2Fmain"), "{asked}");
    assert!(
        asked.contains("key=velnor-tgt-rust-lint-lab-Linux-X64-abcd-"),
        "{asked}"
    );
}

#[test]
fn trim_deletes_nothing_when_the_family_has_no_more_than_the_kept_entries() {
    let entries =
        velnor_ci::cache::parse_entries(&listing()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        stale_entries(&entries, "velnor-tgt-rust-lint-lab-Linux-X64-9999-", 2),
        Vec::<u64>::new()
    );
    assert_eq!(stale_entries(&entries, "nothing-", 2), Vec::<u64>::new());
}
