// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::symlink;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::io::Repo;

type TestResult = Result<(), Box<dyn Error>>;

const TOKEN: &str = "ghs_release_asset_token";
const BYTES: &[u8] = b"the private fixture bytes";
const WRONG: &str = "e4f1a5c1d8b49f5b79a1a4b2a3a8b6f51d8bf9a38a7c1fa5d9d6f1d1a3f6a6c0";

fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut text, byte| {
            write!(text, "{byte:02x}").ok();
            text
        })
}

struct Api {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Api {
    fn seen(&self) -> Vec<String> {
        self.requests.lock().map(|r| r.clone()).unwrap_or_default()
    }
}

fn serve(revoke_status: u16) -> Result<Api, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let url = format!("http://127.0.0.1:{port}");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let text = String::from_utf8_lossy(&buffer[..read]).to_lowercase();
            let first = text.lines().next().unwrap_or_default().to_owned();
            let authorization = text
                .lines()
                .find(|line| line.starts_with("authorization:"))
                .unwrap_or("authorization: none")
                .to_owned();
            if let Ok(mut seen) = seen.lock() {
                seen.push(format!("{first} | {authorization}"));
            }
            let path = first.split_whitespace().nth(1).unwrap_or_default();
            let (head, body): (String, Vec<u8>) = if first.starts_with("delete /installation/token")
            {
                (format!("HTTP/1.1 {revoke_status} X"), Vec::new())
            } else if path.ends_with("/releases/tags/v1") {
                (
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json".to_owned(),
                    br#"{"assets":[{"id":7,"name":"other.bin"},{"id":8,"name":"data.bin"}]}"#
                        .to_vec(),
                )
            } else if path.ends_with("/releases/assets/8") {
                (
                    format!("HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/blob/8"),
                    Vec::new(),
                )
            } else if path == "/blob/8" {
                ("HTTP/1.1 200 OK".to_owned(), BYTES.to_vec())
            } else {
                ("HTTP/1.1 404 Not Found".to_owned(), Vec::new())
            };
            drop(
                stream.write_all(
                    format!(
                        "{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                ),
            );
            drop(stream.write_all(&body));
        }
    });
    Ok(Api { url, requests })
}

fn run(repo: &Repo, api: &Api, token: Option<&str>, dest: &str, sha256: &str) -> Output {
    let entry = format!("acme/fixtures,v1,data.bin,{sha256},{dest}");
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .current_dir(repo.root())
        .args(["fetch-assets", "--asset", &entry])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("VELNOR_API_URL", &api.url);
    if let Some(token) = token {
        command.env("VELNOR_PRIVATE_TOKEN", token);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn revoked(api: &Api) -> bool {
    api.seen().iter().any(|request| {
        request.starts_with("delete /installation/token")
            && request.contains(&format!("bearer {TOKEN}"))
    })
}

fn leftovers(repo: &Repo, dir: &str) -> Vec<String> {
    fs::read_dir(repo.root().join(dir))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn an_asset_with_the_right_digest_is_downloaded_without_the_token_on_the_redirect_and_the_token_is_revoked()
-> TestResult {
    let repo = Repo::empty("assets-good");
    let api = serve(204)?;
    let sha = digest(BYTES);
    let output = run(&repo, &api, Some(TOKEN), "tests/fixtures", &sha);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read(repo.root().join("tests/fixtures/data.bin"))?,
        BYTES
    );
    assert_eq!(leftovers(&repo, "tests/fixtures"), ["data.bin"]);
    let seen = api.seen();
    assert!(
        seen.iter().any(
            |r| r.starts_with("get /repos/acme/fixtures/releases/tags/v1")
                && r.contains(&format!("bearer {TOKEN}"))
        ),
        "{seen:?}"
    );
    let blob = seen
        .iter()
        .find(|r| r.contains("/blob/8"))
        .ok_or("no blob request")?;
    assert!(blob.ends_with("authorization: none"), "{blob}");
    assert!(revoked(&api), "{seen:?}");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(TOKEN));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(TOKEN));
    Ok(())
}

#[test]
fn a_digest_mismatch_fails_leaves_no_file_and_still_revokes_the_token() -> TestResult {
    let repo = Repo::empty("assets-bad-digest");
    let api = serve(204)?;
    let output = run(&repo, &api, Some(TOKEN), "tests/fixtures", WRONG);
    assert!(!output.status.success(), "{output:?}");
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("sha256") && message.contains(WRONG),
        "{message}"
    );
    assert_eq!(leftovers(&repo, "tests/fixtures"), Vec::<String>::new());
    assert!(revoked(&api));
    Ok(())
}

#[test]
fn a_destination_that_escapes_the_workspace_is_refused_before_any_request_but_the_revocation()
-> TestResult {
    let repo = Repo::empty("assets-escape");
    let outside = Repo::empty("assets-outside");
    symlink(outside.root(), repo.root().join("link"))?;
    fs::create_dir_all(repo.root().join("real"))?;
    symlink(outside.root(), repo.root().join("real/inner"))?;
    let sha = digest(BYTES);
    for dest in [
        "../escape",
        "a/../../escape",
        "/tmp/absolute",
        "link",
        "link/sub",
        "real/inner/sub",
        "",
        ".",
        "a//b",
        "a\\b",
    ] {
        let api = serve(204)?;
        let output = run(&repo, &api, Some(TOKEN), dest, &sha);
        assert!(!output.status.success(), "{dest:?}: {output:?}");
        let gets = api
            .seen()
            .iter()
            .filter(|request| request.starts_with("get "))
            .count();
        assert_eq!(gets, 0, "{dest:?} reached the API");
        assert!(revoked(&api), "{dest:?}");
    }
    assert!(
        leftovers(&outside, ".").is_empty(),
        "wrote outside the workspace"
    );
    Ok(())
}

#[test]
fn without_a_token_a_cached_file_with_the_right_digest_passes_offline_and_a_tampered_one_fails()
-> TestResult {
    let repo = Repo::empty("assets-cached");
    let api = serve(204)?;
    let sha = digest(BYTES);
    fs::create_dir_all(repo.root().join("tests/fixtures"))?;
    fs::write(repo.root().join("tests/fixtures/data.bin"), BYTES)?;
    let output = run(&repo, &api, None, "tests/fixtures", &sha);
    assert!(output.status.success(), "{output:?}");
    assert!(api.seen().is_empty(), "a warm run must make no request");
    fs::write(repo.root().join("tests/fixtures/data.bin"), b"tampered")?;
    let output = run(&repo, &api, None, "tests/fixtures", &sha);
    assert!(!output.status.success(), "{output:?}");
    assert_eq!(api.seen(), Vec::<String>::new());
    Ok(())
}

#[test]
fn without_a_token_or_a_cached_file_the_step_fails_closed_naming_the_fork_case() -> TestResult {
    let repo = Repo::empty("assets-fork");
    let api = serve(204)?;
    let output = run(&repo, &api, None, "tests/fixtures", &digest(BYTES));
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("fork"),
        "{output:?}"
    );
    assert_eq!(api.seen(), Vec::<String>::new());
    Ok(())
}

#[test]
fn a_cached_file_that_is_a_symlink_is_not_read() -> TestResult {
    let repo = Repo::empty("assets-cached-link");
    let outside = Repo::empty("assets-cached-target");
    fs::write(outside.root().join("data.bin"), BYTES)?;
    fs::create_dir_all(repo.root().join("tests/fixtures"))?;
    symlink(
        outside.root().join("data.bin"),
        repo.root().join("tests/fixtures/data.bin"),
    )?;
    let api = serve(204)?;
    let output = run(&repo, &api, None, "tests/fixtures", &digest(BYTES));
    assert!(!output.status.success(), "{output:?}");
    Ok(())
}

#[test]
fn a_revocation_that_fails_fails_the_step() -> TestResult {
    let repo = Repo::empty("assets-revoke-fails");
    let api = serve(500)?;
    let output = run(&repo, &api, Some(TOKEN), "tests/fixtures", &digest(BYTES));
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("revoked"),
        "{output:?}"
    );
    Ok(())
}
