// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

use crate::support::fresh_dir;
use serve::{Reply, Served, serve};

mod hardening;
mod serve;

type ResultTest = Result<(), Box<dyn std::error::Error>>;

const LISTING: &str = include_str!("data/fetch_reports/artifacts.json");
const PLAN: &str = include_str!("data/fetch_reports/plan.json");
const FINAL_REPORT: &str = include_str!("data/fetch_reports/final-report.json");
const ARCHIVE: &[u8] = include_bytes!("data/fetch_reports/report-artifact.zip");
const RUN_ID: u64 = 44_491_127_982;
const REPO: &str = "tailrocks/velnor-new";

fn listing_path(page: usize) -> String {
    format!("/repos/{REPO}/actions/runs/{RUN_ID}/artifacts?per_page=100&page={page}")
}

fn listed(listing: &Value) -> Vec<Value> {
    listing["artifacts"].as_array().cloned().unwrap_or_default()
}

fn routes(listing: &Value, archive: &[u8]) -> BTreeMap<String, Vec<Reply>> {
    let mut routes = BTreeMap::new();
    for artifact in listed(listing) {
        let id = artifact["id"].as_u64().unwrap_or_default();
        routes.insert(
            format!("/repos/{REPO}/actions/artifacts/{id}/zip"),
            vec![Reply::redirect(&format!("{{base}}/blob/{id}"))],
        );
        routes.insert(format!("/blob/{id}"), vec![Reply::ok(archive)]);
    }
    let page = json!({"total_count": listed(listing).len(), "artifacts": listed(listing)});
    routes.insert(listing_path(1), vec![Reply::ok(page.to_string())]);
    routes
}

fn recorded() -> Value {
    serde_json::from_str(LISTING).unwrap_or_else(|e| unreachable!("{e}"))
}

fn id_of(listing: &Value, name: &str) -> u64 {
    listed(listing)
        .iter()
        .find(|artifact| artifact["name"] == name)
        .and_then(|artifact| artifact["id"].as_u64())
        .unwrap_or_else(|| unreachable!("{name} is not in the recorded listing"))
}

fn expected_names(plan: &str) -> BTreeSet<String> {
    let plan: Value = serde_json::from_str(plan).unwrap_or_else(|e| unreachable!("{e}"));
    plan["matrix"]["include"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["artifact_id"].as_str().map(str::to_owned))
        .collect()
}

fn run_dir(label: &str, plan: Option<&str>) -> PathBuf {
    let dir = fresh_dir("velnor-ci-fetch-reports", label);
    if let Some(plan) = plan {
        fs::write(dir.join("plan.json"), plan).unwrap_or_else(|e| unreachable!("{e}"));
    }
    dir
}

fn fetch(served: &Served, dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["fetch-reports", "--repo", REPO, "--run-id"])
        .arg(RUN_ID.to_string())
        .arg("--run-dir")
        .arg(dir)
        .env("GITHUB_API_URL", &served.api)
        .env("GITHUB_TOKEN", "fixture-token")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn fetched(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir.join("reports"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let name = path.strip_prefix(root).unwrap_or(&path);
                out.insert(
                    name.to_string_lossy().into_owned(),
                    fs::read(&path).unwrap_or_default(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn archive_members(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).unwrap_or_else(|e| unreachable!("{e}"));
    (0..archive.len())
        .filter_map(|index| {
            let mut member = archive
                .by_index(index)
                .unwrap_or_else(|e| unreachable!("{e}"));
            let mut content = Vec::new();
            member
                .read_to_end(&mut content)
                .unwrap_or_else(|e| unreachable!("{e}"));
            let name = member.enclosed_name()?.to_string_lossy().into_owned();
            Some((name, content))
        })
        .collect()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_recorded_run_yields_the_artifact_set_and_files_the_legacy_fetch_left_for_merge() -> ResultTest
{
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("recorded", Some(PLAN));
    let output = fetch(&served, &dir);
    assert!(output.status.success(), "{output:?}");
    let legacy: Value = serde_json::from_str(FINAL_REPORT)?;
    let downloaded: BTreeSet<String> = legacy["downloaded_artifact_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().map(str::to_owned))
        .collect();
    assert_eq!(downloaded.len(), 31);
    assert_eq!(fetched(&dir), downloaded);
    assert_eq!(expected_names(PLAN), downloaded);
    let members = archive_members(ARCHIVE);
    for name in &downloaded {
        assert_eq!(tree(&dir.join("reports").join(name)), members, "{name}");
    }
    Ok(())
}

#[test]
fn the_run_is_listed_once_and_every_expected_archive_is_downloaded_without_leaking_the_token() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("requests", Some(PLAN));
    assert!(fetch(&served, &dir).status.success());
    assert_eq!(served.count(&format!("/repos/{REPO}/actions/runs")), 1);
    assert_eq!(
        served.count(&format!("/repos/{REPO}/actions/artifacts/")),
        31
    );
    let blobs: Vec<_> = served
        .hits()
        .into_iter()
        .filter(|hit| hit.path.starts_with("/blob/"))
        .collect();
    assert_eq!(blobs.len(), 31);
    assert!(blobs.iter().all(|hit| !hit.authorized), "{blobs:?}");
}

#[test]
fn a_report_that_was_never_uploaded_fails_the_run_after_the_others_are_fetched() {
    let mut listing = recorded();
    let name = "velnor-crate-r44491127982-a1-rust-velnor-gen";
    listing["artifacts"] = Value::Array(
        listed(&listing)
            .into_iter()
            .filter(|artifact| artifact["name"] != name)
            .collect(),
    );
    let served = serve(routes(&listing, ARCHIVE));
    let dir = run_dir("missing", Some(PLAN));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(name) && message.contains("not uploaded"),
        "{message}"
    );
    let mut expected = expected_names(PLAN);
    expected.remove(name);
    assert_eq!(fetched(&dir), expected);
}

#[test]
fn an_expired_report_fails_the_run_and_is_named() {
    let mut listing = recorded();
    let name = "velnor-crate-r44491127982-a1-rust-velnor-ci";
    for artifact in listing["artifacts"].as_array_mut().into_iter().flatten() {
        if artifact["name"] == name {
            artifact["expired"] = json!(true);
        }
    }
    let served = serve(routes(&listing, ARCHIVE));
    let dir = run_dir("expired", Some(PLAN));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(name) && message.contains("expired"),
        "{message}"
    );
    assert!(!fetched(&dir).contains(name));
}

#[test]
fn a_transient_download_failure_is_retried_and_a_persistent_one_fails_after_three_attempts() {
    let listing = recorded();
    let flaky = id_of(&listing, "velnor-crate-r44491127982-a1-rust-velnor-gen");
    let dead = id_of(&listing, "velnor-crate-r44491127982-a1-rust-velnor-ci");
    let mut table = routes(&listing, ARCHIVE);
    table.insert(
        format!("/blob/{flaky}"),
        vec![Reply::status(500), Reply::ok(ARCHIVE)],
    );
    table.insert(format!("/blob/{dead}"), vec![Reply::status(500)]);
    let served = serve(table);
    let dir = run_dir("retries", Some(PLAN));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(served.count(&format!("/blob/{flaky}")), 2);
    assert_eq!(served.count(&format!("/blob/{dead}")), 3);
    let message = stderr(&output);
    assert!(
        message.contains("rust-velnor-ci") && !message.contains("rust-velnor-gen"),
        "{message}"
    );
    assert!(fetched(&dir).contains("velnor-crate-r44491127982-a1-rust-velnor-gen"));
    assert!(!fetched(&dir).contains("velnor-crate-r44491127982-a1-rust-velnor-ci"));
}

#[test]
fn every_page_of_the_listing_is_read() {
    let listing = recorded();
    let all = listed(&listing);
    let (first, rest) = all.split_at(20);
    let mut table = routes(&listing, ARCHIVE);
    let page = |items: &[Value]| json!({"total_count": all.len(), "artifacts": items}).to_string();
    table.insert(listing_path(1), vec![Reply::ok(page(first))]);
    table.insert(listing_path(2), vec![Reply::ok(page(rest))]);
    let served = serve(table);
    let dir = run_dir("pages", Some(PLAN));
    let output = fetch(&served, &dir);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fetched(&dir), expected_names(PLAN));
}

fn hostile_archive() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("../escaped.json", options)
        .unwrap_or_else(|e| unreachable!("{e}"));
    writer
        .write_all(b"{}")
        .unwrap_or_else(|e| unreachable!("{e}"));
    writer
        .finish()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .into_inner()
}

#[test]
fn an_archive_member_that_leaves_the_report_directory_fails_and_writes_nothing() {
    let served = serve(routes(&recorded(), &hostile_archive()));
    let name = "velnor-crate-r44491127982-a1-rust-velnor-gen";
    let plan = json!({"matrix": {"include": [{"artifact_id": name, "matrix_key": "m-0"}]}});
    let dir = run_dir("hostile", Some(&plan.to_string()));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains(name));
    assert!(fetched(&dir).is_empty());
    assert!(!dir.join("escaped.json").exists());
    assert!(!dir.join("reports").join("escaped.json").exists());
}

#[test]
fn a_missing_or_malformed_plan_fails_before_any_request() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("plans", None);
    for (label, plan) in [
        ("absent", None),
        ("garbled", Some("{")),
        ("shapeless", Some("{}")),
    ] {
        if let Some(plan) = plan {
            fs::write(dir.join("plan.json"), plan).unwrap_or_else(|e| unreachable!("{e}"));
        }
        assert_eq!(plan.is_some(), dir.join("plan.json").exists());
        let output = fetch(&served, &dir);
        assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
        assert!(
            stderr(&output).contains("plan.json"),
            "{label}: {}",
            stderr(&output)
        );
    }
    assert_eq!(served.count(""), 0);
}

#[test]
fn an_artifact_name_that_is_not_a_plain_file_name_is_refused() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let plan = json!({"matrix": {"include": [{"artifact_id": "../escape", "matrix_key": "m-0"}]}});
    let dir = run_dir("traversal", Some(&plan.to_string()));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("../escape"));
    assert_eq!(served.count(""), 0);
}
