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
use std::time::SystemTime;

use velnor_ci::io::read_limited;

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

impl Drop for Repo {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
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
