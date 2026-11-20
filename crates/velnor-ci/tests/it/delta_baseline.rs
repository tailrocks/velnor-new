// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

use super::delta::{compiled, manifest, run, set_time, success, time};
use crate::io::Repo;

const INVENTORY: &str = "target/.velnor/delta-inventory.json";

/// What a default-branch run does before it saves the base cache.
fn save_base(repo: &Repo) -> Output {
    repo.write_unstaged("Cargo.lock", "version = 4\n");
    repo.write_unstaged(
        "velnor-run.log",
        "   Compiling app v0.1.0 (/work/app)\n    Finished `dev` profile in 3.20s\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "decide", "--target-dir", "target"])
        .env("RUNNER_TEMP", repo.root())
        .env("GITHUB_OUTPUT", repo.root().join("decide-output"))
        .env("VELNOR_CACHE_MANIFESTS", "Cargo.toml")
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    success(&output);
    assert!(
        fs::read_to_string(repo.root().join("decide-output"))
            .unwrap_or_default()
            .starts_with("save=true\n"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    output
}

#[test]
fn saving_a_base_records_the_digest_of_each_of_its_files() {
    let repo = Repo::new("delta-inventory-recorded");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    save_base(&repo);
    let text = fs::read_to_string(repo.root().join(INVENTORY)).unwrap_or_default();
    assert!(text.contains("debug/deps/libapp.rlib"), "{text}");
}

#[test]
fn a_restored_base_with_an_inventory_is_not_read_again_before_the_first_build() {
    let repo = Repo::new("delta-inventory-no-read");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    let stamp = SystemTime::now() - Duration::from_secs(3600);
    set_time(&repo, "target/debug/deps/libapp.rlib", stamp);
    save_base(&repo);
    // Same size, time and mode, different bytes: only a read of the file can tell.
    repo.write_unstaged("target/debug/deps/libapp.rlib", "RLIB");
    set_time(&repo, "target/debug/deps/libapp.rlib", stamp);
    success(&run(&repo, "apply", "base").0);
    compiled(&repo);
    let (output, saved) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(saved, "save=true\n");
    // apply trusted the recorded digest, so the baseline holds the digest of the old bytes.
    let nodes = manifest(&repo)["nodes"].clone();
    assert!(nodes.get("debug/deps/libapp.rlib").is_some(), "{nodes}");
}

#[test]
fn a_base_without_an_inventory_is_still_read_in_full() {
    let repo = Repo::new("delta-inventory-absent");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    let stamp = SystemTime::now() - Duration::from_secs(3600);
    set_time(&repo, "target/debug/deps/libapp.rlib", stamp);
    success(&run(&repo, "apply", "base").0);
    compiled(&repo);
    let (output, saved) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(saved, "save=true\n");
    let nodes = manifest(&repo)["nodes"].clone();
    assert!(nodes.get("debug/deps/libapp.rlib").is_none(), "{nodes}");
}

#[test]
fn a_file_that_changed_since_the_inventory_is_hashed_afresh_and_belongs_to_the_base() {
    let repo = Repo::new("delta-inventory-changed");
    repo.write_unstaged("target/debug/deps/changed.rlib", "aaaa");
    repo.write_unstaged("target/debug/deps/same.rlib", "same");
    save_base(&repo);
    repo.write_unstaged("target/debug/deps/changed.rlib", "bbbb");
    set_time(
        &repo,
        "target/debug/deps/changed.rlib",
        SystemTime::now() - Duration::from_secs(60),
    );
    success(&run(&repo, "apply", "base").0);
    compiled(&repo);
    let (output, saved) = run(&repo, "stage", "base");
    success(&output);
    assert_eq!(saved, "save=true\n");
    let nodes = manifest(&repo)["nodes"].clone();
    assert!(nodes.get("debug/deps/changed.rlib").is_none(), "{nodes}");
    assert!(nodes.get("debug/deps/same.rlib").is_none(), "{nodes}");
}

const FILES: usize = 500;

fn restored_base(repo: &Repo) {
    let stamp = SystemTime::now() - Duration::from_secs(3600);
    for index in 0..FILES {
        let path = format!("target/debug/deps/lib{index}.rlib");
        repo.write_unstaged(&path, &format!("rlib {index}"));
        set_time(repo, &path, stamp);
    }
    save_base(repo);
}

fn stdout(output: &Output) -> String {
    success(output);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn a_big_restored_base_is_read_only_where_its_files_changed() {
    let repo = Repo::new("delta-inventory-count");
    restored_base(&repo);
    for index in [3, 77, 401] {
        let path = format!("target/debug/deps/lib{index}.rlib");
        repo.write_unstaged(&path, &format!("rebuilt {index}"));
    }
    let report = stdout(&run(&repo, "apply", "base").0);
    assert!(
        report.contains(&format!("read 3 of {FILES} base files\n")),
        "{report}"
    );
}

#[test]
fn staging_reads_only_files_the_build_touched_and_still_sees_hidden_rewrites() {
    let repo = Repo::new("delta-stage-count");
    restored_base(&repo);
    // A change time this recent proves nothing, so let the restored files age past it.
    std::thread::sleep(Duration::from_millis(2200));
    let report = stdout(&run(&repo, "apply", "base").0);
    assert!(
        report.contains(&format!("read 0 of {FILES} base files\n")),
        "{report}"
    );
    compiled(&repo);
    repo.write_unstaged("target/debug/deps/new0.rlib", "new");
    repo.write_unstaged("target/debug/deps/new1.rlib", "new");
    // Same size, same time, other bytes: only the change time can tell.
    let hidden = "target/debug/deps/lib9.rlib";
    let stamp = time(&repo, hidden);
    repo.write_unstaged(hidden, "RLIB 9");
    set_time(&repo, hidden, stamp);
    let (output, saved) = run(&repo, "stage", "base");
    let report = stdout(&output);
    assert_eq!(saved, "save=true\n");
    // The two new outputs, the rewritten file and the inputs record `compiled` saved.
    let expected = format!("read 4 of {} files\n", FILES + 3);
    assert!(report.contains(&expected), "{report}");
    let nodes = manifest(&repo)["nodes"].clone();
    assert!(nodes.get("debug/deps/lib9.rlib").is_some(), "{nodes}");
    assert!(nodes.get("debug/deps/lib10.rlib").is_none(), "{nodes}");
}

/// `prepare` only names the cache key and the source identity, so it must not read the
/// restored base at all. The base files are made unreadable: any read of one fails the
/// command. Root ignores mode bits, so the check is meaningful for a non-root user only.
#[test]
fn prepare_names_the_delta_without_reading_a_restored_base() {
    let repo = Repo::new("delta-prepare-no-read");
    restored_base(&repo);
    for index in 0..FILES {
        let path = repo.root().join(format!("target/debug/deps/lib{index}.rlib"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let (output, prepared) = run(&repo, "prepare", "base");
    success(&output);
    assert!(prepared.starts_with("base8="), "{prepared}");
    assert!(prepared.contains("source_identity="), "{prepared}");
}
