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

mod downloads;
mod refusals;

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
