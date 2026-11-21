// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

const REPO: &str = "tailrocks/velnor-new";
const KEY: &str = "velnor-tools-Linux-X64-mise2026.11.10-aa";
const MAIN: &str = "refs/heads/main";
const MERGE: &str = "refs/pull/7/merge";
const VERSION_A: &str = "a1a1";
const VERSION_B: &str = "b2b2";

struct Served {
    entries: Vec<Value>,
    deleted: Mutex<Vec<String>>,
}

fn entry(id: u64, key: &str, reference: &str, version: &str) -> Value {
    json!({"id":id,"key":key,"ref":reference,"version":version,"created_at":"2026-11-17T00:00:00Z","last_accessed_at":"2026-11-17T00:00:00Z","size_in_bytes":10})
}

fn param(query: &str, name: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
        .map(|text| text.replace("%2F", "/"))
}

fn listing(served: &Served, query: &str) -> String {
    let key = param(query, "key").unwrap_or_default();
    let reference = param(query, "ref");
    let rows: Vec<&Value> = served
        .entries
        .iter()
        .filter(|row| row["key"].as_str().is_some_and(|k| k.starts_with(&key)))
        .filter(|row| reference.as_deref().is_none_or(|r| row["ref"] == r))
        .collect();
    json!({"total_count":rows.len(),"actions_caches":rows}).to_string()
}

fn answer(served: &Served, stream: TcpStream) {
    let Ok(copy) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(copy);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim_end().is_empty() {
            break;
        }
    }
    let body = if method == "DELETE" {
        served
            .deleted
            .lock()
            .unwrap_or_else(|e| unreachable!("{e}"))
            .push(target.clone());
        String::new()
    } else {
        let query = target.split_once('?').map_or("", |(_, query)| query);
        listing(served, query)
    };
    let reply = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = stream;
    stream.write_all(reply.as_bytes()).ok();
}

fn serve(entries: Vec<Value>) -> (String, Arc<Served>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let served = Arc::new(Served {
        entries,
        deleted: Mutex::default(),
    });
    let shared = Arc::clone(&served);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            answer(&shared, stream);
        }
    });
    (format!("http://{address}"), served)
}

fn delete_entry(api: &str, args: &[&str]) -> Output {
    run(api, args, Some(REPO), Some("recorded"))
}

fn run(api: &str, args: &[&str], repo: Option<&str>, token: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.args(["cache", "delete-entry"]);
    if let Some(repo) = repo {
        command.args(["--repo", repo]);
    }
    command
        .args(args)
        .env("GITHUB_API_URL", api)
        .env_remove("GITHUB_REPOSITORY");
    match token {
        Some(token) => command.env("GITHUB_TOKEN", token),
        None => command.env_remove("GITHUB_TOKEN"),
    };
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn deleted(served: &Served) -> Vec<String> {
    served
        .deleted
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .clone()
}

#[test]
fn the_binary_deletes_the_only_entry_its_key_and_ref_name() {
    let (api, served) = serve(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_B),
    ]);
    let output = delete_entry(&api, &["--key", KEY, "--ref", MERGE]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("deleted cache entry ID 12"), "{stdout}");
    assert_eq!(
        deleted(&served),
        [format!("/repos/{REPO}/actions/caches/12")]
    );
}

#[test]
fn two_versions_under_one_key_on_one_ref_are_refused_and_both_are_named() {
    let (api, served) = serve(vec![
        entry(31, KEY, MAIN, VERSION_A),
        entry(32, KEY, MAIN, VERSION_B),
    ]);
    let output = delete_entry(&api, &["--key", KEY, "--ref", MAIN]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("2 entries have the key"), "{stderr}");
    assert!(
        stderr.contains(&format!("version {VERSION_A}"))
            && stderr.contains(&format!("version {VERSION_B}")),
        "{stderr}"
    );
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn a_key_with_no_entry_on_the_ref_is_refused_and_nothing_is_deleted() {
    let (api, served) = serve(vec![entry(51, KEY, MAIN, VERSION_A)]);
    let output = delete_entry(&api, &["--key", KEY, "--ref", MERGE]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(stderr.contains("nothing was deleted"), "{stderr}");
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn an_empty_ref_is_refused_and_nothing_is_deleted() {
    let (api, served) = serve(vec![entry(41, KEY, "", VERSION_A)]);
    let output = delete_entry(&api, &["--key", KEY, "--ref", ""]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("pass the exact cache key and its ref"),
        "{stderr}"
    );
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn an_empty_key_is_refused_and_nothing_is_deleted() {
    let (api, served) = serve(vec![entry(42, "", MAIN, VERSION_A)]);
    let output = delete_entry(&api, &["--key", "", "--ref", MAIN]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("pass the exact cache key and its ref"),
        "{stderr}"
    );
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn a_missing_ref_argument_is_refused_and_nothing_is_deleted() {
    let (api, served) = serve(vec![entry(43, KEY, MAIN, VERSION_A)]);
    let output = delete_entry(&api, &["--key", KEY]);
    assert!(!output.status.success());
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn a_missing_repo_is_refused_and_nothing_is_deleted() {
    let (api, served) = serve(vec![entry(61, KEY, MAIN, VERSION_A)]);
    let output = run(&api, &["--key", KEY, "--ref", MAIN], None, Some("recorded"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("set GITHUB_REPOSITORY or pass --repo owner/name"),
        "{stderr}"
    );
    assert_eq!(deleted(&served), Vec::<String>::new());
}

#[test]
fn a_missing_token_is_refused_and_nothing_is_deleted() {
    let (api, served) = serve(vec![entry(71, KEY, MAIN, VERSION_A)]);
    let output = run(&api, &["--key", KEY, "--ref", MAIN], Some(REPO), None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("set GITHUB_TOKEN to a GitHub token"),
        "{stderr}"
    );
    assert_eq!(deleted(&served), Vec::<String>::new());
}
