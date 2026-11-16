// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use velnor_ci::io::{restore_inputs, save_inputs};

use crate::io::Repo;

#[test]
fn a_retargeted_symlink_restores_nothing() {
    let repo = Repo::new("symlink");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write_all(&[("data/a.txt", "a\n"), ("data/b.txt", "b\n")]);
    repo.link("data/a.txt", "data/link.txt");
    let paths = [".gitignore", "src/lib.rs", "data/a.txt", "data/b.txt"];
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in paths {
        repo.set_mtime(path, built);
    }
    save_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

    fs::remove_file(repo.root().join("data/link.txt")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.link("data/b.txt", "data/link.txt");
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report =
        restore_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("data/a.txt"), checkout);
    assert_eq!(repo.mtime("data/b.txt"), checkout);
    assert_eq!(repo.mtime("src/lib.rs"), checkout);
    assert_eq!(
        report,
        "restored nothing: 1 tracked symlinks are new, changed or deleted since the save\n"
    );
}

#[test]
fn an_unchanged_symlink_does_not_block_restore() {
    let repo = Repo::new("symlink-same");
    repo.write("data/a.txt", "a\n");
    repo.link("data/a.txt", "data/link.txt");
    let paths = [".gitignore", "data/a.txt"];
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in paths {
        repo.set_mtime(path, built);
    }
    save_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report =
        restore_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("data/a.txt"), built);
    assert_eq!(
        report,
        "restored the modification time of 2 unchanged files; 0 changed, 0 new, 0 deleted, 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
    );
}

#[test]
fn a_symlink_the_file_cannot_hold_means_nothing_is_saved() {
    let repo = Repo::new("symlink-odd");
    repo.write("data/a.txt", "a\n");
    repo.link("data/a.txt", "data/odd\tlink");
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    repo.set_mtime("data/a.txt", built);
    let saved =
        save_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

    repo.git(&["rm", "--quiet", "--force", "data/odd\tlink"]);
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    repo.set_mtime("data/a.txt", checkout);
    let report =
        restore_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("data/a.txt"), checkout);
    assert_eq!(
        (saved.as_str(), report.as_str()),
        (
            "saved nothing: 1 tracked symlinks have a path or target the file cannot hold, so restore finds no saved inputs\n",
            "no target/.velnor/inputs.tsv; nothing restored (cold run)\n"
        )
    );
}

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
    save_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

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
    let report =
        restore_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

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
    let saved =
        save_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report =
        restore_inputs(repo.root(), Path::new("target")).unwrap_or_else(|e| unreachable!("{e}"));

    assert_eq!(repo.mtime("sub/src/lib.rs"), built);
    assert_eq!(
        (saved.as_str(), report.as_str()),
        (
            "saved 5 tracked files and 0 symlinks to target/.velnor/inputs.tsv; left out: 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n",
            "restored the modification time of 5 unchanged files; 0 changed, 0 new, 0 deleted, 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
        )
    );
}
