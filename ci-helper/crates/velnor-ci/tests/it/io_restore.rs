// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, UNIX_EPOCH};

use velnor_ci::io::{break_time_ties, restore_inputs, save_inputs};

use crate::io::Repo;

#[test]
fn restore_brings_back_the_saved_time_of_unchanged_files_only() {
    let repo = Repo::new("restore");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write("src/main.rs", "fn main() {}\n");
    repo.write("data/greeting.txt", "hello\n");
    repo.write("src/edited.rs", "pub fn three() {}\n");
    repo.write("tools/gen.sh", "echo gen\n");
    repo.write("src/old.rs", "pub fn old() {}\n");
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_789);
    let before = [
        ".gitignore",
        "src/lib.rs",
        "src/main.rs",
        "data/greeting.txt",
        "src/edited.rs",
        "tools/gen.sh",
    ];
    for path in before {
        repo.set_mtime(path, built);
    }
    save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    repo.write("src/main.rs", "fn main() { println!(\"changed\"); }\n");
    repo.write("src/new.rs", "pub fn three() { todo() }\n");
    repo.make_executable("tools/gen.sh");
    repo.git(&["rm", "--quiet", "--force", "src/old.rs"]);
    fs::write(repo.root().join("src/edited.rs"), "pub fn three() { 3 }\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    for path in before.iter().chain(&["src/new.rs"]) {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("src/lib.rs"), built);
    assert_eq!(repo.mtime("data/greeting.txt"), built);
    assert_eq!(repo.mtime("src/main.rs"), checkout);
    assert_eq!(repo.mtime("src/new.rs"), checkout);
    assert_eq!(repo.mtime("src/edited.rs"), checkout);
    assert_eq!(repo.mtime("tools/gen.sh"), checkout);
    assert_eq!(
        report,
        "restored the modification time of 3 unchanged files; 3 changed, 1 new, 1 deleted, 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
    );
}

#[test]
fn a_saved_file_that_sparse_checkout_left_out_is_counted_not_an_error() {
    let repo = Repo::new("sparse");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write("assets/data.txt", "one\n");
    repo.commit();
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in [".gitignore", "src/lib.rs", "assets/data.txt"] {
        repo.set_mtime(path, built);
    }
    save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    repo.git(&["update-index", "-q", "--refresh"]);
    repo.git(&["sparse-checkout", "set", "--no-cone", "/src/"]);
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    repo.set_mtime("src/lib.rs", checkout);
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    assert!(!repo.root().join("assets/data.txt").exists());
    assert_eq!(repo.mtime("src/lib.rs"), built);
    assert_eq!(
        report,
        "restored the modification time of 1 unchanged files; 0 changed, 0 new, 0 deleted, 2 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
    );
}

#[test]
fn a_file_checked_out_with_other_bytes_keeps_its_checkout_time() {
    let repo = Repo::new("eol");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write("assets/data.txt", "one\ntwo\n");
    repo.commit();
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in [".gitignore", "src/lib.rs", "assets/data.txt"] {
        repo.set_mtime(path, built);
    }
    save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    repo.write(".gitattributes", "*.txt text eol=crlf\n");
    repo.commit();
    fs::remove_file(repo.root().join("assets/data.txt")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.git(&["checkout", "--", "assets/data.txt"]);
    assert_eq!(
        fs::read(repo.root().join("assets/data.txt")).unwrap_or_else(|e| unreachable!("{e}")),
        b"one\r\ntwo\r\n"
    );
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in [
        ".gitignore",
        "src/lib.rs",
        "assets/data.txt",
        ".gitattributes",
    ] {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("assets/data.txt"), checkout);
    assert_eq!(repo.mtime("src/lib.rs"), built);
    assert_eq!(
        report,
        "restored the modification time of 2 unchanged files; 1 changed, 1 new, 0 deleted, 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
    );
}

#[test]
fn a_mode_changed_only_in_the_worktree_keeps_its_checkout_time() {
    let repo = Repo::new("chmod");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write("tools/run.sh", "echo run\n");
    repo.write("keys/key.txt", "key\n");
    let paths = [".gitignore", "src/lib.rs", "tools/run.sh", "keys/key.txt"];
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in paths {
        repo.set_mtime(path, built);
    }
    save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    for (path, mode) in [("tools/run.sh", 0o755), ("keys/key.txt", 0o600)] {
        fs::set_permissions(repo.root().join(path), Permissions::from_mode(mode))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("tools/run.sh"), checkout);
    assert_eq!(repo.mtime("keys/key.txt"), checkout);
    assert_eq!(repo.mtime("src/lib.rs"), built);
    assert_eq!(
        report,
        "restored the modification time of 2 unchanged files; 2 changed, 0 new, 0 deleted, 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
    );
}

#[test]
fn a_workspace_below_the_repository_root_is_saved_and_restored() {
    let repo = Repo::new("nested");
    repo.write("rust/src/lib.rs", "pub fn one() {}\n");
    repo.write("shared/src/lib.rs", "pub fn shared() {}\n");
    repo.write("other/src/lib.rs", "pub fn other() {}\n");
    repo.write(".velnor/rust-inputs.lock", "shared\trust-app\n");
    let workspace = repo.root().join("rust");
    let paths = [
        ".gitignore",
        "rust/src/lib.rs",
        "shared/src/lib.rs",
        "other/src/lib.rs",
        ".velnor/rust-inputs.lock",
    ];
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in paths {
        repo.set_mtime(path, built);
    }
    let saved = save_inputs(&workspace).unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(&workspace).unwrap_or_else(|e| unreachable!("{e}"));

    assert!(workspace.join("target/.velnor/inputs.tsv").is_file());
    assert_eq!(repo.mtime("rust/src/lib.rs"), built);
    assert_eq!(repo.mtime("shared/src/lib.rs"), built);
    assert_eq!(repo.mtime("other/src/lib.rs"), checkout);
    assert_eq!(repo.mtime(".gitignore"), checkout);
    assert_eq!(
        (saved.as_str(), report.as_str()),
        (
            "saved 2 tracked files and 0 symlinks to target/.velnor/inputs.tsv; left out: 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n",
            "restored the modification time of 2 unchanged files; 0 changed, 0 new, 0 deleted, 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
        )
    );
}

#[test]
fn a_nested_root_reading_the_repository_root_keeps_the_times_of_files_outside_it() {
    let repo = Repo::new("nested-root-scope");
    repo.write("rust/src/lib.rs", "pub fn one() {}\n");
    repo.write("other/src/lib.rs", "pub fn other() {}\n");
    repo.write(".velnor/rust-inputs.lock", ".\trust-app\n");
    let workspace = repo.root().join("rust");
    let paths = ["rust/src/lib.rs", "other/src/lib.rs"];
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in paths {
        repo.set_mtime(path, built);
    }
    save_inputs(&workspace).unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    restore_inputs(&workspace).unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("rust/src/lib.rs"), built);
    assert_eq!(repo.mtime("other/src/lib.rs"), built);
}

#[test]
fn two_roots_restored_from_different_snapshots_each_keep_their_own_times() {
    let repo = Repo::new("two-roots");
    repo.write("one/src/lib.rs", "pub fn one() {}\n");
    repo.write("two/src/lib.rs", "pub fn two() {}\n");
    let paths = ["one/src/lib.rs", "two/src/lib.rs"];
    let (first, second) = (
        UNIX_EPOCH + Duration::new(1_700_000_000, 0),
        UNIX_EPOCH + Duration::new(1_700_000_500, 0),
    );
    for path in paths {
        repo.set_mtime(path, first);
    }
    save_inputs(&repo.root().join("one")).unwrap_or_else(|e| unreachable!("{e}"));
    for path in paths {
        repo.set_mtime(path, second);
    }
    save_inputs(&repo.root().join("two")).unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    for root in ["one", "two"] {
        restore_inputs(&repo.root().join(root)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    assert_eq!(repo.mtime("one/src/lib.rs"), first);
    assert_eq!(repo.mtime("two/src/lib.rs"), second);
}

#[test]
fn restore_without_saved_inputs_changes_nothing() {
    let repo = Repo::new("cold");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    repo.set_mtime("src/lib.rs", checkout);
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(repo.mtime("src/lib.rs"), checkout);
    assert_eq!(
        report,
        "no target/.velnor/inputs.tsv; nothing restored (cold run)\n"
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
    save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in [".gitignore", "src/lib.rs", "build.rs"] {
        repo.set_mtime(path, checkout);
    }
    restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
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
