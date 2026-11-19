// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Cases for the qualification runs, the producer runs, jobs and artifacts, and the
//! downloaded archives.

use serde_json::{Value, json};

use crate::admit_cases::{Case, cases, push, put};
use crate::admit_world::{AdmitWorld, MAIN};
use crate::admit_zip::{crc32, zip_of};
use crate::preview_world::{SIGNER, SOURCE, darwin_payload, linux_payload};

const LINUX: &str = "velnor-actions-x86_64-unknown-linux-gnu";
const DARWIN: &str = "velnor-actions-aarch64-apple-darwin";
const FILE: u32 = 0o100_644;

fn artifacts(run: u64) -> String {
    format!("/actions/runs/{run}/artifacts?per_page=100")
}

fn jobs(run: u64) -> String {
    format!("/actions/runs/{run}/attempts/1/jobs?per_page=100")
}

fn ci_runs() -> String {
    format!(
        "/actions/workflows/ci.yml/runs?head_sha={SOURCE}&event=push&status=success&per_page=20"
    )
}

fn ci_run(world: &mut AdmitWorld, pointer: &str, value: Value) {
    put(
        world,
        &ci_runs(),
        &format!("/workflow_runs/0{pointer}"),
        value,
    );
}

fn run800(world: &mut AdmitWorld, pointer: &str, value: Value) {
    put(world, "/actions/runs/800/attempts/1", pointer, value);
}

fn twice(world: &mut AdmitWorld, suffix: &str, list: &str, twin: Option<Value>) {
    let first = world.api[&AdmitWorld::path(suffix)]
        .pointer(&format!("{list}/0"))
        .cloned()
        .unwrap_or_default();
    push(world, suffix, list, twin.unwrap_or(first));
    put(world, suffix, "/total_count", json!(2));
}

fn hundred(world: &mut AdmitWorld) {
    let many: Vec<Value> = (1..=100)
        .map(|i| json!({"id": 6000 + i, "name": "Required verdict"}))
        .collect();
    put(world, &jobs(600), "/jobs", json!(many));
    put(world, &jobs(600), "/total_count", json!(100));
}

fn linux_zip(world: &mut AdmitWorld, name: &str, data: &[u8], flags: u16, mode: u32) {
    world.rezip("linux", zip_of(&[(name, data)], flags, mode));
}

fn darwin_zip(world: &mut AdmitWorld, data: &[u8]) {
    world.rezip("darwin", zip_of(&[(DARWIN, data)], 0, FILE));
}

fn flip(world: &mut AdmitWorld) {
    let mut zip = world.zips[&11].clone();
    zip[40] ^= 1;
    world.zips.insert(11, zip);
}

fn bad_checksum(world: &mut AdmitWorld) {
    let data = linux_payload();
    let mut zip = zip_of(&[(LINUX, &data)], 0, FILE);
    let central = 30 + LINUX.len() + data.len() + 16;
    zip[central..central + 4].copy_from_slice(&crc32(b"x").to_le_bytes());
    world.rezip("linux", zip);
}

fn with(mut data: Vec<u8>, at: usize, byte: u8) -> Vec<u8> {
    data[at] = byte;
    data
}

pub(crate) const PRODUCERS: &[Case] = cases! {
    "no successful ci run" => |w| put(w, &ci_runs(), "/workflow_runs", json!([]));
    "ci run commit" => |w| ci_run(w, "/head_sha", json!(MAIN));
    "ci run branch" => |w| ci_run(w, "/head_branch", json!("dev"));
    "ci run event" => |w| ci_run(w, "/event", json!("pull_request"));
    "ci run workflow" => |w| ci_run(w, "/path", json!(".github/workflows/other.yml"));
    "ci run ref" => |w| ci_run(w, "/path", json!(".github/workflows/ci.yml@refs/heads/dev"));
    "ci run conclusion" => |w| ci_run(w, "/conclusion", json!("failure"));
    "ci run status" => |w| ci_run(w, "/status", json!("in_progress"));
    "ci run repository id" => |w| ci_run(w, "/head_repository/id", json!(1));
    "ci run id is not a number" => |w| ci_run(w, "/id", json!("600"));
    "ci jobs are missing" => |w| put(w, &jobs(600), "/jobs", json!([]));
    "ci job failed" => |w| put(w, &jobs(600), "/jobs/0/conclusion", json!("failure"));
    "ci job is running" => |w| put(w, &jobs(600), "/jobs/0/status", json!("in_progress"));
    "ci job is another name" => |w| put(w, &jobs(600), "/jobs/0/name", json!("Other"));
    "ci job is for another run" => |w| put(w, &jobs(600), "/jobs/0/run_id", json!(1));
    "ci job is for another attempt" => |w| put(w, &jobs(600), "/jobs/0/run_attempt", json!(2));
    "ci job is for another commit" => |w| put(w, &jobs(600), "/jobs/0/head_sha", json!(MAIN));
    "ci job count" => |w| put(w, &jobs(600), "/total_count", json!(2));
    "ci jobs are 100" => |w| hundred(w);
    "ci job is twice" => |w| twice(w, &jobs(600), "/jobs", Some(json!({"id": 6002, "name": "Required verdict", "run_id": 600, "run_attempt": 1, "head_sha": SOURCE, "status": "completed", "conclusion": "success"})));
    "ci jobs share an id" => |w| twice(w, &jobs(600), "/jobs", None);
    "qualification job failed" => |w| put(w, &jobs(700), "/jobs/3/conclusion", json!("cancelled"));
    "producer run commit" => |w| run800(w, "/head_sha", json!(MAIN));
    "producer run branch" => |w| run800(w, "/head_branch", json!("dev"));
    "producer run event" => |w| run800(w, "/event", json!("schedule"));
    "producer run status" => |w| run800(w, "/status", json!("queued"));
    "producer run workflow" => |w| run800(w, "/path", json!(format!("{SIGNER}x@main")));
    "producer run repository" => |w| run800(w, "/head_repository/full_name", json!("evil/repo"));
    "darwin producer run attempt" => |w| put(w, "/actions/runs/900/attempts/1", "/run_attempt", json!(2));
    "producer build job failed" => |w| put(w, &jobs(800), "/jobs/0/conclusion", json!("failure"));
    "producer qualify job failed" => |w| put(w, &jobs(900), "/jobs/1/conclusion", json!("failure"));
    "producer qualify job differs from the annotation" => |w| w.annotate(|r| r["targets"]["linux"]["qualification_job_id"] = json!(1));
    "producer has no artifact" => |w| put(w, &artifacts(800), "/artifacts", json!([]));
    "producer artifact count" => |w| put(w, &artifacts(800), "/total_count", json!(2));
    "producer artifact is twice" => |w| twice(w, &artifacts(800), "/artifacts", None);
    "producer artifact name" => |w| put(w, &artifacts(800), "/artifacts/0/name", json!("source-preview-payload-linux-2"));
    "producer artifact expired" => |w| put(w, &artifacts(800), "/artifacts/0/expired", json!(true));
    "producer artifact id differs from the annotation" => |w| put(w, &artifacts(800), "/artifacts/0/id", json!(99));
    "producer artifact digest differs from the annotation" => |w| put(w, &artifacts(900), "/artifacts/0/digest", json!(format!("sha256:{}", "a".repeat(64))));
    "producer artifact digest is malformed" => |w| put(w, &artifacts(900), "/artifacts/0/digest", json!("sha256:abc"));
    "producer artifact is of another run" => |w| put(w, &artifacts(800), "/artifacts/0/workflow_run/id", json!(1));
    "producer artifact is of another commit" => |w| put(w, &artifacts(800), "/artifacts/0/workflow_run/head_sha", json!(MAIN));
    "producer artifact size is wrong" => |w| put(w, &artifacts(800), "/artifacts/0/size_in_bytes", json!(1));
    "producer artifact is empty" => |w| put(w, &artifacts(800), "/artifacts/0/size_in_bytes", json!(0));
};

pub(crate) const ARCHIVES: &[Case] = cases! {
    "archive entry name" => |w| linux_zip(w, "other", &linux_payload(), 0, FILE);
    "archive is encrypted" => |w| linux_zip(w, LINUX, &linux_payload(), 1, FILE);
    "archive entry is a directory" => |w| linux_zip(w, LINUX, &linux_payload(), 0, 0o040_755);
    "archive has two entries" => |w| w.rezip("linux", zip_of(&[(LINUX, &linux_payload()), ("x", b"y")], 0, FILE));
    "archive has no entry" => |w| w.rezip("linux", zip_of(&[], 0, FILE));
    "archive is not a zip" => |w| w.rezip("linux", b"not a zip".to_vec());
    "archive differs from its digest" => |w| flip(w);
    "payload is not an ELF" => |w| linux_zip(w, LINUX, &[0; 64], 0, FILE);
    "payload is a 32-bit ELF" => |w| linux_zip(w, LINUX, &with(linux_payload(), 4, 1), 0, FILE);
    "payload is for another machine" => |w| linux_zip(w, LINUX, &with(linux_payload(), 18, 0xb7), 0, FILE);
    "payload is under 20 bytes" => |w| linux_zip(w, LINUX, &linux_payload()[..19], 0, FILE);
    "darwin payload is not Mach-O" => |w| darwin_zip(w, &[7; 64]);
    "darwin payload is for another cpu" => |w| darwin_zip(w, &with(darwin_payload(), 4, 7));
    "darwin payload is a short header" => |w| darwin_zip(w, &darwin_payload()[..31]);
    "archive entry has a bad checksum" => |w| bad_checksum(w);
};
