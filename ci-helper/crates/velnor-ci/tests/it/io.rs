// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes, Permissions};
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use velnor_ci::io::{read_limited, restore_inputs, save_inputs};

#[test]
fn a_body_at_the_limit_is_read_whole() {
    let body =
        read_limited(Cursor::new(vec![7_u8; 16]), 16).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(body.len(), 16);
}

#[test]
fn a_body_over_the_limit_is_an_error_not_a_truncation() {
    let result = read_limited(Cursor::new(vec![7_u8; 17]), 16).map(|bytes| bytes.len());
    assert_eq!(
        result.map_err(|e| e.to_string()),
        Err("response is larger than 16 bytes".to_owned())
    );
}

struct Repo(PathBuf);

impl Repo {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("velnor-ci-inputs-{}-{name}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        }
        fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
        let repo = Self(root);
        repo.git(&["init", "--quiet"]);
        repo.write(".gitignore", "/target/\n");
        repo
    }

    fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success(), "git {args:?}");
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(path, text).unwrap_or_else(|e| unreachable!("{e}"));
        self.git(&["add", "--all"]);
    }

    fn commit(&self) {
        self.git(&[
            "-c",
            "user.name=lab",
            "-c",
            "user.email=lab@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--message",
            "commit",
        ]);
    }

    fn make_executable(&self, path: &str) {
        fs::set_permissions(self.0.join(path), Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        self.git(&["add", "--all"]);
    }

    fn set_mtime(&self, path: &str, time: SystemTime) {
        File::options()
            .write(true)
            .open(self.0.join(path))
            .and_then(|file| file.set_times(FileTimes::new().set_modified(time)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn mtime(&self, path: &str) -> SystemTime {
        fs::metadata(self.0.join(path))
            .and_then(|metadata| metadata.modified())
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn root(&self) -> &Path {
        &self.0
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

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
        "restored the modification time of 3 unchanged files; 3 changed, 1 new, 1 deleted, 0 not in the worktree, 0 with a path the file cannot hold\n"
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
        "restored the modification time of 1 unchanged files; 0 changed, 0 new, 0 deleted, 2 not in the worktree, 0 with a path the file cannot hold\n"
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
        "restored the modification time of 2 unchanged files; 1 changed, 1 new, 0 deleted, 0 not in the worktree, 0 with a path the file cannot hold\n"
    );
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
