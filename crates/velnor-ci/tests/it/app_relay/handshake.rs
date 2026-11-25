// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};
use std::time::Instant;

use serde_json::json;
use zip::write::SimpleFileOptions;

use crate::fetch_reports::serve::{Reply, Served, serve};
use crate::support::fresh_dir;

const RUN_ID: &str = "77";
const REPO: &str = "acme/app";
const MINTED: &str = "ghs_mintedInstallationToken0123456789";
const PEM: &str = include_str!("../data/app_token/TEST-ONLY-not-a-secret-rsa-pkcs1.pem");
const LISTING: &str = "/repos/acme/app/actions/runs/77/artifacts?per_page=100&page=1";

struct Job {
    dir: velnor_test_support::ScratchDir,
    attempt: &'static str,
}

impl Job {
    fn new(label: &str, attempt: &'static str) -> Self {
        Self {
            dir: fresh_dir("velnor-ci-app-relay", label),
            attempt,
        }
    }

    fn output(&self) -> std::path::PathBuf {
        self.dir.join("github-output")
    }

    fn key_file(&self, job: &str) -> std::path::PathBuf {
        self.dir.join(format!("velnor-app-token/{job}.key"))
    }

    fn run(&self, served: Option<&Served>, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .env_clear()
            .arg("app-token")
            .args(args)
            .env("GITHUB_REPOSITORY", REPO)
            .env("GITHUB_RUN_ID", RUN_ID)
            .env("GITHUB_RUN_ATTEMPT", self.attempt)
            .env("RUNNER_TEMP", self.dir.path())
            .env("GITHUB_OUTPUT", self.output());
        if let Some(served) = served {
            command
                .env("GITHUB_API_URL", &served.api)
                .env("VELNOR_API_URL", &served.api)
                .env("GITHUB_TOKEN", "job-token")
                .env("VELNOR_APP_CLIENT_ID", "Iv23liTestClient")
                .env("VELNOR_APP_PRIVATE_KEY", PEM);
        }
        command.output().unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn outputs(&self) -> String {
        fs::read_to_string(self.output()).unwrap_or_default()
    }
}

fn archive(member: &str, content: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(member, SimpleFileOptions::default())
        .unwrap_or_else(|e| unreachable!("{e}"));
    writer
        .write_all(content)
        .unwrap_or_else(|e| unreachable!("{e}"));
    writer
        .finish()
        .unwrap_or_else(|e| unreachable!("{e}"))
        .into_inner()
}

fn artifacts(uploaded: &[(&str, &str, Vec<u8>)]) -> Served {
    let mut routes = BTreeMap::new();
    let mut listed = Vec::new();
    for (index, (name, member, content)) in uploaded.iter().enumerate() {
        let id = 100 + index;
        listed.push(json!({"id": id, "name": name, "expired": false}));
        routes.insert(
            format!("/repos/acme/app/actions/artifacts/{id}/zip"),
            vec![Reply::ok(archive(member, content))],
        );
    }
    let page = json!({"total_count": listed.len(), "artifacts": listed});
    routes.insert(LISTING.to_owned(), vec![Reply::ok(page.to_string())]);
    routes.insert(
        "/repos/acme/shared-lib/installation".to_owned(),
        vec![Reply::ok(json!({"id": 4242}).to_string())],
    );
    routes.insert(
        "/app/installations/4242/access_tokens".to_owned(),
        vec![Reply::ok(json!({"token": MINTED}).to_string())],
    );
    routes.insert("/installation/token".to_owned(), vec![Reply::status(204)]);
    serve(routes)
}

fn request(job: &Job, name: &str) -> Vec<u8> {
    let out = job.run(None, &["request", "--job", name]);
    assert!(out.status.success(), "{}", stderr(&out));
    fs::read(
        job.dir
            .join(format!("velnor-app-token/{name}/public-key.json")),
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

fn relay(job: &Job, served: &Served, name: &str, wait: &[&str]) -> Output {
    let mut args = vec![
        "relay",
        "--job",
        name,
        "--owner=acme",
        "--repository=shared-lib",
    ];
    args.extend_from_slice(wait);
    job.run(Some(served), &args)
}

fn sealed_of(job: &Job, name: &str) -> Vec<u8> {
    fs::read(
        job.dir
            .join(format!("velnor-app-token/{name}/token.sealed")),
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn minted(served: &Served) -> usize {
    served.count("/app/installations/4242/access_tokens")
}

#[test]
fn the_building_job_receives_the_token_that_the_relay_sealed_for_its_key() {
    let building = Job::new("building", "1");
    let relaying = Job::new("relaying", "1");
    let public = request(&building, "release-pr");
    let mode = fs::metadata(building.key_file("release-pr"))
        .map_or_default(|meta| meta.permissions().mode() & 0o777);
    assert_eq!(mode, 0o600);
    assert!(
        building
            .outputs()
            .contains("artifact=velnor-app-key-release-pr-77-1")
    );
    let served = artifacts(&[("velnor-app-key-release-pr-77-1", "public-key.json", public)]);
    let out = relay(&relaying, &served, "release-pr", &[]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        relaying
            .outputs()
            .contains("artifact=velnor-app-token-release-pr-77-1")
    );
    let sealed = sealed_of(&relaying, "release-pr");
    assert!(!String::from_utf8_lossy(&sealed).contains(MINTED));
    assert!(!String::from_utf8_lossy(&out.stdout).contains(MINTED));
    let served = artifacts(&[("velnor-app-token-release-pr-77-1", "token.sealed", sealed)]);
    let out = building.run(Some(&served), &["await", "--job", "release-pr"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains(&format!("::add-mask::{MINTED}")));
    assert!(building.outputs().contains(&format!("token={MINTED}")));
    assert!(!building.key_file("release-pr").exists());
}

#[test]
fn a_sealed_token_of_another_attempt_gives_the_job_no_token() {
    let first = Job::new("first-attempt", "1");
    let relaying = Job::new("first-relay", "1");
    let public = request(&first, "release-pr");
    let served = artifacts(&[("velnor-app-key-release-pr-77-1", "public-key.json", public)]);
    assert!(
        relay(&relaying, &served, "release-pr", &[])
            .status
            .success()
    );
    let sealed = sealed_of(&relaying, "release-pr");
    let retry = Job::new("second-attempt", "2");
    request(&retry, "release-pr");
    let served = artifacts(&[("velnor-app-token-release-pr-77-2", "token.sealed", sealed)]);
    let out = retry.run(Some(&served), &["await", "--job", "release-pr"]);
    assert!(!out.status.success());
    assert!(!retry.outputs().contains("token="));
    assert!(!retry.key_file("release-pr").exists());
}

#[test]
fn a_sealed_token_for_another_key_of_the_same_run_gives_the_job_no_token() {
    let building = Job::new("own-key", "1");
    let other = Job::new("other-key", "1");
    let relaying = Job::new("other-relay", "1");
    request(&building, "tag");
    let public = request(&other, "tag");
    let served = artifacts(&[("velnor-app-key-tag-77-1", "public-key.json", public)]);
    assert!(relay(&relaying, &served, "tag", &[]).status.success());
    let sealed = sealed_of(&relaying, "tag");
    let served = artifacts(&[("velnor-app-token-tag-77-1", "token.sealed", sealed)]);
    let out = building.run(Some(&served), &["await", "--job", "tag"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("does not open"));
    assert!(!building.outputs().contains("token="));
}

#[test]
fn the_relay_mints_nothing_for_a_public_key_bound_to_another_job() {
    let building = Job::new("foreign", "1");
    let relaying = Job::new("foreign-relay", "1");
    let public = request(&building, "tag");
    let served = artifacts(&[("velnor-app-key-release-pr-77-1", "public-key.json", public)]);
    let out = relay(&relaying, &served, "release-pr", &[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("job identity"));
    assert_eq!(minted(&served), 0);
}

#[test]
fn the_relay_mints_nothing_when_two_artifacts_claim_the_public_key_name() {
    let building = Job::new("squatted", "1");
    let relaying = Job::new("squatted-relay", "1");
    let public = request(&building, "tag");
    let name = "velnor-app-key-tag-77-1";
    let served = artifacts(&[
        (name, "public-key.json", public.clone()),
        (name, "public-key.json", public),
    ]);
    let out = relay(&relaying, &served, "tag", &[]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("another job in this run"));
    assert_eq!(minted(&served), 0);
}

#[test]
fn waiting_for_the_sealed_token_is_bounded_and_deletes_the_private_key() {
    let building = Job::new("timeout", "1");
    request(&building, "tag");
    let served = artifacts(&[]);
    let started = Instant::now();
    let out = building.run(
        Some(&served),
        &[
            "await",
            "--job",
            "tag",
            "--timeout-seconds=1",
            "--poll-seconds=1",
        ],
    );
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("no artifact velnor-app-token-tag-77-1 appeared within 1 seconds")
    );
    assert!(started.elapsed().as_secs() < 30);
    assert!(!building.key_file("tag").exists());
    assert!(!building.outputs().contains("token="));
}

#[test]
fn the_relay_gives_up_when_no_public_key_arrives() {
    let relaying = Job::new("no-request", "1");
    let served = artifacts(&[]);
    let out = relay(
        &relaying,
        &served,
        "tag",
        &["--timeout-seconds=1", "--poll-seconds=1"],
    );
    assert!(!out.status.success());
    assert!(stderr(&out).contains("no artifact velnor-app-key-tag-77-1 appeared"));
    assert_eq!(minted(&served), 0);
}
