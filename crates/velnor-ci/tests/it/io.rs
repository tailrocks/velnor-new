// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs::{self, File, FileTimes, Permissions};
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU64;
use std::thread;
use std::time::SystemTime;

use velnor_ci::io::read_limited;

use crate::support::{DEAD_PID, assert_dead, fresh_dir, fresh_dir_in};

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

#[test]
fn repos_with_the_same_name_in_one_run_do_not_share_a_folder() {
    let first = Repo::new("same-name");
    let second = Repo::new("same-name");
    first.write("first.txt", "first\n");
    second.write("second.txt", "second\n");
    assert_ne!(first.root(), second.root());
    assert!(first.root().join("first.txt").is_file());
    assert!(second.root().join("second.txt").is_file());
}

#[test]
fn a_folder_left_by_an_earlier_run_is_not_removed_by_a_new_repo() {
    let leftover =
        Repo(std::env::temp_dir().join(format!("velnor-ci-inputs-{}-audit", std::process::id())));
    fs::create_dir_all(leftover.root()).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(leftover.root().join("marker.txt"), "left behind\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let repo = Repo::new("audit");
    repo.write("a.txt", "a\n");
    assert!(leftover.root().join("marker.txt").is_file());
}

fn isolated_root(label: &str) -> Repo {
    Repo(fresh_dir("velnor-ci-test-root", label))
}

fn sweep_in(root: &Repo) -> PathBuf {
    fresh_dir_in(root.root(), &AtomicU64::new(0), "velnor-ci-inputs", "sweep")
}

fn make(root: &Repo, name: &str) -> PathBuf {
    let folder = root.root().join(name);
    fs::create_dir_all(&folder).unwrap_or_else(|e| unreachable!("{e}"));
    folder
}

#[test]
fn a_folder_of_a_process_that_is_gone_is_removed_and_one_of_a_live_process_is_kept() {
    let root = isolated_root("sweep-gone");
    let stale = make(&root, "velnor-ci-inputs-2000000000-0-stale");
    let init = make(&root, "velnor-ci-inputs-1-0-init");
    let own = make(
        &root,
        &format!("velnor-ci-inputs-{}-live-own", std::process::id()),
    );
    sweep_in(&root);
    assert!(!stale.exists());
    assert!(init.is_dir());
    assert!(own.is_dir());
}

#[test]
fn a_folder_with_no_dash_after_its_pid_is_left_alone() {
    assert_dead(DEAD_PID);
    let root = isolated_root("sweep-bare");
    let folder = make(&root, &format!("velnor-ci-inputs-{DEAD_PID}"));
    sweep_in(&root);
    assert!(folder.is_dir());
}

#[test]
fn a_folder_with_an_empty_rest_after_its_pid_dash_is_left_alone() {
    assert_dead(DEAD_PID);
    let root = isolated_root("sweep-empty");
    let folder = make(&root, &format!("velnor-ci-inputs-{DEAD_PID}-"));
    sweep_in(&root);
    assert!(folder.is_dir());
}

#[test]
fn a_dead_pid_folder_that_only_resembles_the_prefix_is_left_alone() {
    assert_dead(DEAD_PID);
    let root = isolated_root("sweep-resembles");
    let contains = make(&root, &format!("x-velnor-ci-inputs-{DEAD_PID}-a"));
    let longer = make(&root, &format!("velnor-ci-inputs-x-{DEAD_PID}-a"));
    sweep_in(&root);
    assert!(contains.is_dir());
    assert!(longer.is_dir());
}

#[test]
fn a_repo_folder_is_removed_when_the_repo_is_dropped() {
    let root = {
        let repo = Repo::new("dropped");
        repo.write("a.txt", "a\n");
        repo.root().to_path_buf()
    };
    assert!(!root.exists());
}

#[test]
fn a_repo_folder_is_removed_when_a_panic_unwinds_past_a_live_repo() {
    let mut root = None;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let repo = Repo::new("panicked");
        repo.write("a.txt", "a\n");
        root = Some(repo.root().to_path_buf());
        panic!("unwinding past a live repo");
    }));
    assert!(outcome.is_err());
    let root = root.unwrap_or_else(|| unreachable!("the closure ran"));
    assert!(!root.exists());
}

#[test]
fn a_name_taken_by_a_folder_is_skipped_and_the_folder_is_left_untouched() {
    let root = isolated_root("collision");
    let taken = make(
        &root,
        &format!("velnor-ci-inputs-{}-0-skipme", std::process::id()),
    );
    fs::write(taken.join("marker.txt"), "taken\n").unwrap_or_else(|e| unreachable!("{e}"));
    let counter = AtomicU64::new(0);
    let created = fresh_dir_in(root.root(), &counter, "velnor-ci-inputs", "skipme");
    assert_ne!(created, taken);
    assert!(created.ends_with(format!("velnor-ci-inputs-{}-1-skipme", std::process::id())));
    assert_eq!(
        fs::read_to_string(taken.join("marker.txt")).unwrap_or_default(),
        "taken\n"
    );
}

pub(crate) struct Repo(PathBuf);

impl Repo {
    pub(crate) fn empty(name: &str) -> Self {
        Self(fresh_dir("velnor-ci-inputs", name))
    }

    pub(crate) fn new(name: &str) -> Self {
        let repo = Self::empty(name);
        repo.git(&["init", "--quiet"]);
        repo.write_unstaged(".gitignore", "/target/\n");
        repo
    }

    pub(crate) fn new_under(parent: &Path, name: &str) -> Self {
        let repo = Self(fresh_dir_in(
            parent,
            &AtomicU64::new(0),
            "velnor-ci-inputs",
            name,
        ));
        repo.git(&["init", "--quiet"]);
        repo.write_unstaged(".gitignore", "/target/\n");
        repo
    }

    pub(crate) fn fork(&self, name: &str) -> Self {
        let copy = Self::empty(name);
        copy_tree(&self.0, &copy.0);
        copy
    }

    pub(crate) fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success(), "git {args:?}");
    }

    pub(crate) fn write_unstaged(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap_or_else(|| unreachable!()))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(path, text).unwrap_or_else(|e| unreachable!("{e}"));
    }

    pub(crate) fn write_all(&self, files: &[(&str, &str)]) {
        for (path, text) in files {
            self.write_unstaged(path, text);
        }
        self.git(&["add", "--all"]);
    }

    pub(crate) fn write(&self, path: &str, text: &str) {
        self.write_all(&[(path, text)]);
    }

    pub(crate) fn commit(&self) {
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

    pub(crate) fn link(&self, target: &str, path: &str) {
        let target = Path::new(target)
            .file_name()
            .unwrap_or_else(|| unreachable!());
        std::os::unix::fs::symlink(target, self.0.join(path))
            .unwrap_or_else(|e| unreachable!("{e}"));
        self.git(&["add", "--all"]);
    }

    pub(crate) fn make_executable(&self, path: &str) {
        fs::set_permissions(self.0.join(path), Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        self.git(&["add", "--all"]);
    }

    pub(crate) fn set_mtime(&self, path: &str, time: SystemTime) {
        File::options()
            .write(true)
            .open(self.0.join(path))
            .and_then(|file| file.set_times(FileTimes::new().set_modified(time)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    pub(crate) fn mtime(&self, path: &str) -> SystemTime {
        fs::metadata(self.0.join(path))
            .and_then(|metadata| metadata.modified())
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    pub(crate) fn root(&self) -> &Path {
        &self.0
    }
}

fn copy_tree(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap_or_else(|e| unreachable!("{e}")) {
        let entry = entry.unwrap_or_else(|e| unreachable!("{e}"));
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            fs::create_dir_all(&target).unwrap_or_else(|e| unreachable!("{e}"));
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap_or_else(|e| unreachable!("{e}"));
        }
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn serve(responses: BTreeMap<String, String>) -> String {
    serve_with_status(
        responses
            .into_iter()
            .map(|(path, body)| (path, (200, body)))
            .collect(),
    )
}

pub(crate) fn serve_with_status(responses: BTreeMap<String, (u16, String)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                request.push(byte[0]);
            }
            let text = String::from_utf8_lossy(&request).into_owned();
            let path = text.split_whitespace().nth(1).unwrap_or_default();
            let (code, body) = responses
                .get(path)
                .map_or((404, String::new()), |(code, body)| (*code, body.clone()));
            let status = match code {
                200 => "200 OK".to_owned(),
                404 => "404 Not Found".to_owned(),
                other => format!("{other} Error"),
            };
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).ok();
        }
    });
    format!("http://{address}")
}
