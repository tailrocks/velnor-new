// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::thread;

use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, GitHubWrite};

use crate::release_fake::{FakeReleases, REPO, TAG, all_files, published_digests};
use crate::release_record::{CI, NOTHING};
use crate::support::fresh_dir;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        Self(fresh_dir("velnor-ci-release", name))
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap_or_else(|e| unreachable!("{e}"));
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ignored = fs::remove_dir_all(&self.0);
    }
}

fn answer(fake: &FakeReleases, api: &str, stream: &TcpStream) -> Result<String, CiError> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| CiError::new(e.to_string()))?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers = BTreeMap::new();
    loop {
        let mut header = String::new();
        reader
            .read_line(&mut header)
            .map_err(|e| CiError::new(e.to_string()))?;
        let Some((key, value)) = header.trim_end().split_once(':') else {
            break;
        };
        headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
    }
    let length = headers
        .get("content-length")
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| CiError::new(e.to_string()))?;
    if headers.get("authorization").map(String::as_str) != Some("Bearer recorded") {
        return Err(CiError::new("401 Unauthorized"));
    }
    let content = headers.get("content-type").map(String::as_str);
    match (method.as_str(), content) {
        ("GET", _) => fake.get(&path),
        ("POST", Some("application/octet-stream")) => fake.upload(&format!("{api}{path}"), &body),
        ("PATCH", Some("application/json")) => fake.patch(&path, &String::from_utf8_lossy(&body)),
        ("DELETE", _) => fake.delete(&path).map(|()| String::new()),
        _ => Err(CiError::new(format!("415 {method} {path} {content:?}"))),
    }
}

fn serve() -> (String, Arc<FakeReleases>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let api = format!("http://{address}");
    let fake = Arc::new(FakeReleases::uploading_to(&api));
    let served = Arc::clone(&fake);
    let base = api.clone();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let (status, body) = match answer(&served, &base, &stream) {
                Ok(body) => ("200 OK", body),
                Err(error) => ("422 Unprocessable Entity", error.to_string()),
            };
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).ok();
        }
    });
    (api, fake)
}

fn velnor_ci(args: &[&str], env: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.args(args).env("GITHUB_TOKEN", "recorded");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn publish(api: &Path, dir: &Dir) -> Output {
    let dir = dir.0.to_string_lossy().into_owned();
    let mut args = vec![
        "release",
        "publish",
        "--repo",
        REPO,
        "--tag",
        TAG,
        "--app",
        "velnor-release",
        "--dir",
        &dir,
    ];
    args.extend(all_files().iter().map(|(name, _)| *name).take(3));
    velnor_ci(&args, &[("GITHUB_API_URL", api)])
}

#[test]
fn a_folder_named_by_pid_alone_is_not_removed_by_a_new_dir() {
    let leftover =
        Dir(std::env::temp_dir().join(format!("velnor-ci-release-{}-publish", std::process::id())));
    fs::create_dir_all(&leftover.0).unwrap_or_else(|e| unreachable!("{e}"));
    leftover.write("marker.txt", "left behind\n");
    let dir = Dir::new("publish");
    assert_ne!(dir.0, leftover.0);
    assert!(leftover.0.join("marker.txt").is_file());
}

#[test]
fn release_record_appends_one_line_per_released_package_to_github_output() {
    let dir = Dir::new("record");
    let ci = dir.write("ci-helper.json", CI);
    let generator = dir.write("generator.json", NOTHING);
    let outputs = dir.write("github-output", "ready=true\n");
    let paths = [ci.to_string_lossy(), generator.to_string_lossy()];
    let output = velnor_ci(
        &["release", "record", &paths[0], &paths[1]],
        &[("GITHUB_OUTPUT", &outputs)],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "velnor-ci=velnor-ci-v0.1.80\n"
    );
    assert_eq!(
        fs::read_to_string(&outputs).unwrap_or_default(),
        "ready=true\nvelnor-ci=velnor-ci-v0.1.80\n"
    );

    let output = velnor_ci(
        &["release", "record", &paths[0], &paths[0]],
        &[("GITHUB_OUTPUT", &outputs)],
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        fs::read_to_string(&outputs).unwrap_or_default(),
        "ready=true\nvelnor-ci=velnor-ci-v0.1.80\n"
    );
}

#[test]
fn release_publish_uploads_over_http_and_un_drafts_the_app_draft() {
    let (api, fake) = serve();
    let id = fake.app_draft();
    let dir = Dir::new("publish");
    for (name, text) in all_files() {
        dir.write(name, text);
    }
    let output = publish(Path::new(&api), &dir);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        stdout.ends_with(&format!("published release {TAG} ({id}) with 4 files\n")),
        "{stdout}"
    );
    assert!(!fake.is_draft(id));
    assert_eq!(fake.assets(id), published_digests());
}

#[test]
fn release_publish_exits_1_and_changes_nothing_when_a_file_is_missing_on_disk() {
    let (api, fake) = serve();
    let id = fake.app_draft();
    let dir = Dir::new("publish-missing");
    for (name, text) in all_files() {
        if name != "velnor-ci-macos-arm64" {
            dir.write(name, text);
        }
    }
    let output = publish(Path::new(&api), &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("velnor-ci-macos-arm64: missing"),
        "{output:?}"
    );
    assert!(fake.is_draft(id));
    assert_eq!(fake.assets(id), BTreeMap::new());
}
