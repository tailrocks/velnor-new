// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use velnor_ci::io::{break_time_ties, restore_inputs, save_inputs};

use crate::io::Repo;

#[test]
fn a_symlink_retargeted_inside_a_submodule_restores_nothing() {
    let origin = Repo::new("submodule-origin");
    std::os::unix::fs::symlink("../data/a.txt", origin.root().join("link.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    origin.git(&["add", "--all"]);
    origin.commit();

    let repo = Repo::new("submodule");
    repo.write_all(&[("data/a.txt", "a\n"), ("data/b.txt", "b\n")]);
    let origin_path = origin.root().to_str().unwrap_or_else(|| unreachable!());
    repo.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "--quiet",
        "add",
        origin_path,
        "sub",
    ]);
    repo.commit();
    let paths = [".gitignore", "data/a.txt", "data/b.txt"];
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in paths {
        repo.set_mtime(path, built);
    }
    save_inputs(repo.root(), std::path::Path::new("target"))
        .unwrap_or_else(|e| unreachable!("{e}"));

    let sub = repo.root().join("sub");
    fs::remove_file(sub.join("link.txt")).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink("../data/b.txt", sub.join("link.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["-C", "sub", "add", "--all"]);
    repo.git(&[
        "-C",
        "sub",
        "-c",
        "user.name=lab",
        "-c",
        "user.email=lab@example.com",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--quiet",
        "--message",
        "retarget",
    ]);
    repo.git(&["add", "sub"]);
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(repo.root(), std::path::Path::new("target"))
        .err()
        .unwrap_or_else(|| unreachable!("changed symlink was accepted"))
        .to_string();

    assert_eq!(repo.mtime("data/a.txt"), checkout);
    assert_eq!(repo.mtime("data/b.txt"), checkout);
    assert_eq!(
        report,
        "restored nothing: 1 tracked symlinks are new, changed or deleted since the save\n"
    );
}

#[test]
fn files_inside_a_submodule_are_restored_when_unchanged() {
    let origin = Repo::new("submodule-files-origin");
    origin.write("src/lib.rs", "pub fn sub() {}\n");
    origin.commit();

    let repo = Repo::new("submodule-files");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let origin_path = origin.root().to_str().unwrap_or_else(|| unreachable!());
    repo.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "--quiet",
        "add",
        origin_path,
        "sub",
    ]);
    repo.commit();
    let paths = [".gitignore", ".gitmodules", "src/lib.rs", "sub/src/lib.rs"];
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in paths {
        repo.set_mtime(path, built);
    }
    let saved = save_inputs(repo.root(), std::path::Path::new("target"))
        .unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(repo.root(), std::path::Path::new("target"))
        .unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("sub/src/lib.rs"), built);
    assert_eq!(
        (saved.as_str(), report.as_str()),
        (
            "saved 5 tracked files and 0 symlinks to target/.velnor/inputs.tsv; left out: 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n",
            "restored the modification time of 5 unchanged files; 0 changed, 0 new, 0 deleted, 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
        )
    );
}

#[test]
fn restore_without_saved_inputs_refuses_cached_artifacts_and_changes_no_sources() {
    let repo = Repo::new("cold");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    repo.set_mtime("src/lib.rs", checkout);
    let report = restore_inputs(repo.root(), std::path::Path::new("target"))
        .err()
        .unwrap_or_else(|| unreachable!("missing snapshot was accepted"))
        .to_string();
    assert_eq!(repo.mtime("src/lib.rs"), checkout);
    assert_eq!(
        report,
        "no target/.velnor/inputs.tsv; cannot restore cached inputs"
    );
}

#[test]
fn files_that_share_a_modification_time_get_distinct_ones_so_cargo_never_picks_the_newest_by_chance()
 {
    let repo = Repo::new("ties");
    for path in ["build.rs", "src/lib.rs", "src/main.rs", "other.txt"] {
        repo.write(path, "x\n");
    }
    let tied = UNIX_EPOCH + Duration::new(1_700_000_000, 193_657_627);
    let alone = UNIX_EPOCH + Duration::new(1_700_000_100, 5);
    for path in [".gitignore", "build.rs", "src/lib.rs", "src/main.rs"] {
        repo.set_mtime(path, tied);
    }
    repo.set_mtime("other.txt", alone);
    let report = break_time_ties(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        report,
        "gave 3 tracked files a modification time no other tracked file has, out of 5\n"
    );
    assert_eq!(repo.mtime(".gitignore"), tied);
    assert_eq!(repo.mtime("other.txt"), alone);
    let step = Duration::from_nanos(1_000);
    assert_eq!(repo.mtime("build.rs"), tied - step);
    assert_eq!(repo.mtime("src/lib.rs"), tied - 2 * step);
    assert_eq!(repo.mtime("src/main.rs"), tied - 3 * step);
    let again = break_time_ties(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        again,
        "gave 0 tracked files a modification time no other tracked file has, out of 5\n"
    );
}

#[test]
fn restore_then_tie_breaking_leaves_the_restored_times_of_distinct_files_alone() {
    let repo = Repo::new("ties-after-restore");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write("build.rs", "fn main() {}\n");
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789);
    let later = UNIX_EPOCH + Duration::new(1_700_000_000, 223_456_789);
    repo.set_mtime(".gitignore", built);
    repo.set_mtime("src/lib.rs", later);
    repo.set_mtime("build.rs", built - Duration::from_secs(1));
    save_inputs(repo.root(), std::path::Path::new("target"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in [".gitignore", "src/lib.rs", "build.rs"] {
        repo.set_mtime(path, checkout);
    }
    restore_inputs(repo.root(), std::path::Path::new("target"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    break_time_ties(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(repo.mtime("src/lib.rs"), later);
    assert_eq!(repo.mtime(".gitignore"), built);
    assert_eq!(repo.mtime("build.rs"), built - Duration::from_secs(1));
}

#[test]
fn the_restored_command_prints_its_report_and_writes_only_the_expired_output() {
    let repo = Repo::new("restored-command");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("restored")
        .current_dir(repo.root())
        .env("GITHUB_ENV", repo.root().join("env"))
        .env("GITHUB_OUTPUT", repo.root().join("output"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("gave "));
    assert_eq!(output.stderr.len(), 0);
    assert!(!repo.root().join("env").exists());
    assert_eq!(
        std::fs::read_to_string(repo.root().join("output")).unwrap_or_default(),
        "expired=false\n"
    );
}

#[test]
fn configured_target_input_timestamps_round_trip_through_the_cli() {
    let repo = Repo::new("configured-input-times");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789);
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    repo.set_mtime("src/lib.rs", built);
    for verb in ["save", "restore"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["inputs", verb, "--target-dir", "out"])
            .current_dir(repo.root())
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        if verb == "save" {
            assert!(repo.root().join("out/.velnor/inputs.tsv").is_file());
            assert!(!repo.root().join("target/.velnor/inputs.tsv").exists());
            repo.set_mtime("src/lib.rs", checkout);
        }
    }
    assert_eq!(repo.mtime("src/lib.rs"), built);
}
