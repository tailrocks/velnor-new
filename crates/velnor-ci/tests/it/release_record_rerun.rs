// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use velnor_ci::github::{release_by_tag_path, tag_ref_path};

use crate::support::repo;

const MERGE: &str = "1111111111111111111111111111111111111111";
const PLZ: &str = "[workspace]\ngit_tag_name = \"{{ package }}-v{{ version }}\"\ngit_release_draft = true\nrelease_always = false\nsemver_check = false\npublish_no_verify = true\n";
const ONLY_TOOLS: &str =
    r#"{"releases":[{"package_name":"tools","prs":[],"tag":"tools-v0.1.0","version":"0.1.0"}]}"#;
const NOTHING: &str = r#"{"releases":[]}"#;

static CHECKOUTS: AtomicUsize = AtomicUsize::new(0);

struct Checkout(PathBuf);

impl Checkout {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "velnor-ci-rerun-{}-{}",
            std::process::id(),
            CHECKOUTS.fetch_add(1, Ordering::Relaxed)
        ));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap_or_else(|e| unreachable!("{e}"));
        let checkout = Self(dir);
        for (folder, name) in [("", "app"), ("tools", "tools")] {
            checkout.write(
                &join(folder, "Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
            );
            checkout.write(&join(folder, "src/lib.rs"), "");
            checkout.write(&join(folder, "release-plz.toml"), PLZ);
        }
        checkout
    }

    fn write(&self, path: &str, text: &str) -> PathBuf {
        let file = self.0.join(path);
        fs::create_dir_all(file.parent().unwrap_or(Path::new("")))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(&file, text).unwrap_or_else(|e| unreachable!("{e}"));
        file
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn join(folder: &str, file: &str) -> String {
    if folder.is_empty() {
        file.to_owned()
    } else {
        format!("{folder}/{file}")
    }
}

fn serve(responses: BTreeMap<String, String>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let api = format!(
        "http://{}",
        listener
            .local_addr()
            .unwrap_or_else(|e| unreachable!("{e}"))
    );
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut line = String::new();
            let mut reader = BufReader::new(&stream);
            reader.read_line(&mut line).ok();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).ok();
                if header.trim_end().is_empty() {
                    break;
                }
            }
            let path = line.split_whitespace().nth(1).unwrap_or_default();
            let (status, body) = responses
                .get(path)
                .map_or(("404 Not Found", "{}"), |body| ("200 OK", body.as_str()));
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).ok();
        }
    });
    api
}

fn commit(sha: &str) -> String {
    format!(r#"{{"object":{{"type":"commit","sha":"{sha}"}}}}"#)
}

fn draft(author: &str, kind: &str) -> String {
    format!(
        r#"[{{"id":7,"tag_name":"app-v0.1.0","draft":true,"author":{{"login":"{author}","type":"{kind}"}},"upload_url":"x"}}]"#
    )
}

fn api(author: &str, kind: &str) -> String {
    let repo = repo();
    let mut responses = BTreeMap::new();
    for tag in ["app-v0.1.0", "tools-v0.1.0"] {
        responses.insert(tag_ref_path(&repo, tag), commit(MERGE));
    }
    responses.insert(
        "/repos/tailrocks/velnor-new/releases?per_page=100&page=1".to_owned(),
        draft(author, kind),
    );
    let _ = release_by_tag_path(&repo, "app-v0.1.0");
    serve(responses)
}

fn rerun(checkout: &Checkout, api: &str, output: &Path) -> Output {
    let first = checkout.write("release-0.json", NOTHING);
    let second = checkout.write("release-1.json", ONLY_TOOLS);
    let toolchain = std::env::var("RUSTUP_TOOLCHAIN").unwrap_or_else(|_| "stable".to_owned());
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "record", "--repo", "tailrocks/velnor-new"])
        .args(["--verified-commit", MERGE, "--app", "velnor-release"])
        .args([
            "--config",
            "release-plz.toml",
            "--config",
            "tools/release-plz.toml",
        ])
        .arg(first)
        .arg(second)
        .current_dir(&checkout.0)
        .env("GITHUB_TOKEN", "recorded")
        .env("GITHUB_API_URL", api)
        .env("GITHUB_OUTPUT", output)
        .env("RUSTUP_TOOLCHAIN", toolchain)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_rerun_after_a_failed_second_root_records_the_draft_the_first_root_created() {
    let checkout = Checkout::new();
    let output = checkout.0.join("github-output");
    let api = api("velnor-release[bot]", "Bot");
    let result = rerun(&checkout, &api, &output);
    assert_eq!(result.status.code(), Some(0), "{result:?}");
    assert_eq!(
        fs::read_to_string(&output).unwrap_or_default(),
        format!("app=app-v0.1.0\ntools=tools-v0.1.0\napp_commit={MERGE}\ntools_commit={MERGE}\n")
    );
}

#[test]
fn a_rerun_records_nothing_when_the_draft_belongs_to_someone_else() {
    let checkout = Checkout::new();
    let output = checkout.0.join("github-output");
    let api = api("tailrocks-reviewer", "User");
    let result = rerun(&checkout, &api, &output);
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("was created by tailrocks-reviewer (User), not by velnor-release[bot]")
    );
    assert_eq!(fs::read_to_string(&output).unwrap_or_default(), "");
}
