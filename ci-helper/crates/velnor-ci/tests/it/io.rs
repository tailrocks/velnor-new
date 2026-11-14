// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs::{self, File, FileTimes, Permissions};
use std::io::{Cursor, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use velnor_ci::io::{read_limited, restore_inputs, save_inputs};

use crate::gate::{OFF_MAIN_GREEN, OFF_MAIN_RED, ON_MAIN_GREEN, ON_MAIN_RED};
use crate::support::GATE;

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

pub(crate) struct Repo(PathBuf);

impl Repo {
    pub(crate) fn new(name: &str) -> Self {
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

    pub(crate) fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success(), "git {args:?}");
    }

    pub(crate) fn write(&self, path: &str, text: &str) {
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

    fn link(&self, target: &str, path: &str) {
        let target = Path::new(target)
            .file_name()
            .unwrap_or_else(|| unreachable!());
        std::os::unix::fs::symlink(target, self.0.join(path))
            .unwrap_or_else(|e| unreachable!("{e}"));
        self.git(&["add", "--all"]);
    }

    fn make_executable(&self, path: &str) {
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

    fn mtime(&self, path: &str) -> SystemTime {
        fs::metadata(self.0.join(path))
            .and_then(|metadata| metadata.modified())
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    pub(crate) fn root(&self) -> &Path {
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
    let workspace = repo.root().join("rust");
    let paths = [".gitignore", "rust/src/lib.rs", "shared/src/lib.rs"];
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
    assert_eq!(
        (saved.as_str(), report.as_str()),
        (
            "saved 3 tracked files and 0 symlinks to target/.velnor/inputs.tsv; left out: 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n",
            "restored the modification time of 3 unchanged files; 0 changed, 0 new, 0 deleted, 0 not in the worktree, 0 with a path the file cannot hold, 0 submodules not checked out\n"
        )
    );
}

#[test]
fn a_retargeted_symlink_restores_nothing() {
    let repo = Repo::new("symlink");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write("data/a.txt", "a\n");
    repo.write("data/b.txt", "b\n");
    repo.link("data/a.txt", "data/link.txt");
    let paths = [".gitignore", "src/lib.rs", "data/a.txt", "data/b.txt"];
    let built = UNIX_EPOCH + Duration::new(1_700_000_000, 0);
    for path in paths {
        repo.set_mtime(path, built);
    }
    save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    fs::remove_file(repo.root().join("data/link.txt")).unwrap_or_else(|e| unreachable!("{e}"));
    repo.link("data/b.txt", "data/link.txt");
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

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
    save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

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
    let saved = save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    repo.git(&["rm", "--quiet", "--force", "data/odd\tlink"]);
    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    repo.set_mtime("data/a.txt", checkout);
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

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
    repo.write("data/a.txt", "a\n");
    repo.write("data/b.txt", "b\n");
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
    save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

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
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

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
    let saved = save_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

    let checkout = UNIX_EPOCH + Duration::new(1_800_000_000, 0);
    for path in paths {
        repo.set_mtime(path, checkout);
    }
    let report = restore_inputs(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));

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

fn serve(responses: BTreeMap<String, String>) -> String {
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
            let (status, body) = responses
                .get(path)
                .map_or(("404 Not Found", String::new()), |body| {
                    ("200 OK", body.clone())
                });
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).ok();
        }
    });
    format!("http://{address}")
}

fn release_gate(api: &str, token: Option<&str>, sha: &str) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args([
            "release-gate",
            "--repo",
            "tailrocks/velnor-new",
            "--sha",
            sha,
        ])
        .env("GITHUB_API_URL", api);
    match token {
        Some(token) => command.env("GITHUB_TOKEN", token),
        None => command.env_remove("GITHUB_TOKEN"),
    };
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn release_gate_exits_0_for_a_released_commit_and_1_for_each_refusal() {
    let recorded: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(GATE).unwrap_or_else(|e| unreachable!("{e}"));
    let api = serve(
        recorded
            .into_iter()
            .map(|(path, body)| (path, body.to_string()))
            .collect(),
    );
    let cases = [
        (ON_MAIN_GREEN, Some(0), "check Required passed", ""),
        (
            OFF_MAIN_GREEN,
            Some(1),
            "",
            "not on the default branch main",
        ),
        (ON_MAIN_RED, Some(1), "", "it concluded failure"),
        (OFF_MAIN_RED, Some(1), "", "not on the default branch main"),
    ];
    for (sha, code, said, complained) in cases {
        let output = release_gate(&api, Some("recorded"), sha);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), code, "{sha}: {stdout}{stderr}");
        assert!(stdout.contains(said), "{sha}: {stdout}");
        assert!(stderr.contains(complained), "{sha}: {stderr}");
    }
}

#[test]
fn release_gate_without_a_token_exits_1_before_any_request() {
    let output = release_gate("http://127.0.0.1:1", None, ON_MAIN_GREEN);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("set GITHUB_TOKEN"));
}

#[test]
fn check_tag_exits_0_for_the_tag_of_its_own_version_and_1_for_any_other() {
    let version = env!("CARGO_PKG_VERSION");
    for (tag, code) in [
        (format!("rust-ci-v{version}"), Some(0)),
        ("rust-ci-v0.0.0".to_owned(), Some(1)),
        (version.to_owned(), Some(1)),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["check-tag", &tag])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(output.status.code(), code, "{tag}");
    }
}

fn plan_repo(name: &str) -> Repo {
    let repo = Repo::new(name);
    repo.write("mise.toml", "[tools]\nrust = \"1.99.1\"\n");
    repo.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"a\", \"b\"]\nexclude = [\"tools\"]\nresolver = \"3\"\n",
    );
    repo.write("Cargo.lock", "version = 4\n");
    for member in ["a", "b"] {
        repo.write(
            &format!("{member}/Cargo.toml"),
            &format!("[package]\nname = \"{member}\"\nedition = \"2024\"\n"),
        );
        repo.write(&format!("{member}/src/lib.rs"), "");
    }
    repo.write(
        "tools/Cargo.toml",
        "[package]\nname = \"t\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\n",
    );
    repo.write("tools/Cargo.lock", "version = 4\n");
    repo.write("tools/src/main.rs", "fn main() {}\n");
    repo
}

#[test]
fn plan_appends_the_outputs_of_every_root_to_github_output_and_explains_on_request() {
    let repo = plan_repo("plan");
    let outputs = repo.root().join("github-output");
    fs::write(&outputs, "earlier=kept\n").unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("b"))
        .args(["plan", "--explain", "--changed", "a/src/lib.rs"])
        .env("GITHUB_OUTPUT", &outputs)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Cargo.toml: some\n  a changed:a/src/lib.rs\ntools/Cargo.toml: all:most-packages:1\n  t depends-on:a\n"
    );
    assert_eq!(
        fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}")),
        "earlier=kept\nrust_root_mode=some\nrust_root_packages=a\nrust_root_test_filter=package(a)\nrust_tools_mode=all\nrust_tools_packages=\nrust_tools_test_filter=all()\n"
    );
}

fn explain(repo: &Repo, changed: &str) -> String {
    explain_all(repo, &[changed])
}

fn explain_all(repo: &Repo, changed: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["plan", "--explain", "--changed"])
        .args(changed)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn with_an_empty_config_a_root_readme_selects_nothing_and_a_toolchain_file_selects_everything() {
    let repo = plan_repo("classes");
    repo.write(".velnor/rust-ci.toml", "");
    repo.write("README.md", "# readme\n");
    assert_eq!(
        explain(&repo, "README.md"),
        "Cargo.toml: none\ntools/Cargo.toml: none\n"
    );
    assert_eq!(
        explain(&repo, "rust-toolchain.toml"),
        "Cargo.toml: all:toolchain-file:rust-toolchain.toml\ntools/Cargo.toml: all:toolchain-file:rust-toolchain.toml\n"
    );
    let unknown = explain(&repo, "scripts/gen.sh");
    assert!(
        unknown.starts_with(
            "Cargo.toml: all:unknown-path:scripts/gen.sh\ntools/Cargo.toml: all:unknown-path:scripts/gen.sh\n"
        ),
        "{unknown}"
    );
    assert_eq!(unknown.matches("[[inputs]]").count(), 1, "{unknown}");
    let masked = explain_all(
        &repo,
        &[".cargo/config.toml", "scripts/gen.sh", "tools/gen.sh"],
    );
    assert!(
        masked.contains("unknown path: scripts/gen.sh\n"),
        "{masked}"
    );
    assert!(masked.contains("unknown path: tools/gen.sh\n"), "{masked}");
    assert!(unknown.contains("non_inputs"), "{unknown}");
    assert!(unknown.contains("\"scripts/gen.sh\","), "{unknown}");
    assert!(unknown.contains("[[inputs]]"), "{unknown}");
}

fn plan_lock(fnv: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.0.0\"\n\n[[package]]\nname = \"b\"\nversion = \"0.0.0\"\ndependencies = [\"fnv\"]\n\n[[package]]\nname = \"fnv\"\nversion = \"{fnv}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"sum-{fnv}\"\n"
    )
}

fn plan_with_bases(repo: &Repo, changed: &[&str], bases: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .current_dir(repo.root())
        .args(["plan", "--explain", "--changed"])
        .args(changed);
    for (path, text) in bases {
        let file = repo.root().join(format!("base-{}", path.replace('/', "-")));
        fs::write(&file, text).unwrap_or_else(|e| unreachable!("{e}"));
        command
            .arg("--base-file")
            .arg(format!("{path}={}", file.display()));
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn plan_reads_the_old_text_of_a_lockfile_and_selects_only_the_members_whose_dependencies_changed() {
    let repo = plan_repo("lock");
    repo.write("Cargo.lock", &plan_lock("1.0.6"));
    let output = plan_with_bases(
        &repo,
        &["Cargo.lock"],
        &[("Cargo.lock", &plan_lock("1.0.5"))],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Cargo.toml: some\n  b lock:fnv\ntools/Cargo.toml: none\n"
    );
    let without_base = plan_with_bases(&repo, &["Cargo.lock"], &[]);
    assert_eq!(
        String::from_utf8_lossy(&without_base.stdout),
        "Cargo.toml: all:workspace-manifest\ntools/Cargo.toml: all:workspace-manifest\n"
    );
    let missing = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["plan", "--changed", "Cargo.lock", "--base-file"])
        .arg("Cargo.lock=/nonexistent/old-lock")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("cannot read /nonexistent/old-lock"),
        "{missing:?}"
    );
}

#[test]
fn plan_leaves_a_root_manifest_that_only_changed_dependency_versions_to_the_lockfile() {
    let repo = plan_repo("manifest");
    let manifest = |version: &str| {
        format!(
            "[workspace]\nmembers = [\"a\", \"b\"]\nexclude = [\"tools\"]\nresolver = \"3\"\n\n[workspace.dependencies]\nfnv = \"{version}\"\n"
        )
    };
    repo.write("Cargo.toml", &manifest("1.0.6"));
    repo.write("Cargo.lock", &plan_lock("1.0.5"));
    let bases = [
        ("Cargo.toml", manifest("1.0.5")),
        ("Cargo.lock", plan_lock("1.0.5")),
    ];
    let bases: Vec<(&str, &str)> = bases.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let output = plan_with_bases(&repo, &["Cargo.toml", "Cargo.lock"], &bases);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Cargo.toml: none\ntools/Cargo.toml: none\n"
    );
    let alone = plan_with_bases(&repo, &["Cargo.toml"], &bases[..1]);
    assert_eq!(
        String::from_utf8_lossy(&alone.stdout),
        "Cargo.toml: all:workspace-manifest\ntools/Cargo.toml: all:workspace-manifest\n"
    );
    repo.write(
        "Cargo.toml",
        &manifest("1.0.6").replace("resolver = \"3\"", "resolver = \"2\""),
    );
    let output = plan_with_bases(&repo, &["Cargo.toml", "Cargo.lock"], &bases);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Cargo.toml: all:workspace-manifest\ntools/Cargo.toml: all:workspace-manifest\n"
    );
}
