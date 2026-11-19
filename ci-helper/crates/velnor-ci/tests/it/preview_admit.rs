// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `admit-preview`: what it admits, what it asks GitHub in which order, every check it
//! refuses on, and the files it leaves.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;
use velnor_ci::io::preview::admit::publish;
use velnor_ci::preview::admit::{Admission, admit};
use velnor_ci::preview::authority::modes::Inputs;
use velnor_ci::preview::json::{Res, canonical};

use crate::admit_cases::all;
use crate::admit_world::{AdmitWorld, MAIN, tag};
use crate::preview_world::{R, REPO, SIGNER, SOURCE, TAG_OID, darwin_payload, linux_payload, sha};
use crate::support::fresh_dir;

/// What the helper makes of `world`, and the requests it made.
pub(crate) fn helper(world: &AdmitWorld) -> (Res<Admission>, Vec<String>) {
    let client = world.client();
    let result = admit(
        &client,
        &Inputs {
            vars: world.vars.clone(),
        },
    );
    let calls = client.calls.borrow().clone();
    (result, calls)
}

pub(crate) fn tree(root: &Path) -> BTreeMap<String, (Vec<u8>, u32)> {
    fs::read_dir(root)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .map(|entry| {
            let mode = entry
                .metadata()
                .map_or(0, |m| m.permissions().mode() & 0o777);
            let body = fs::read(entry.path()).unwrap_or_default();
            (
                entry.file_name().to_string_lossy().into_owned(),
                (body, mode),
            )
        })
        .collect()
}

fn directory(name: &str) -> PathBuf {
    fs::canonicalize(fresh_dir("velnor-ci-admit", name)).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_valid_tag_push_is_admitted_after_these_requests_in_this_order() {
    let world = AdmitWorld::new();
    let (result, calls) = helper(&world);
    assert!(result.is_ok(), "{:?}", result.err());
    let expected: Vec<String> = [
        "", "/contents/Cargo.toml?ref=1111111111111111111111111111111111111111",
        "USER", "/actions/runs/5000", "/rulesets/26696396", "/rulesets/24397132",
        &format!("/git/ref/tags/{}", tag()), &format!("/git/tags/{TAG_OID}"),
        "/git/ref/heads/main", &format!("/compare/{SOURCE}...{MAIN}"),
        &format!("/actions/workflows/ci.yml/runs?head_sha={SOURCE}&event=push&status=success&per_page=20"),
        "/actions/runs/600/attempts/1/jobs?per_page=100",
        &format!("/actions/workflows/source-qualification.yml/runs?head_sha={SOURCE}&event=workflow_dispatch&status=success&per_page=20"),
        "/actions/runs/700/attempts/1/jobs?per_page=100",
        "/actions/runs/800/attempts/1", "/actions/runs/800/attempts/1/jobs?per_page=100",
        "/actions/runs/800/artifacts?per_page=100",
        "/actions/runs/900/attempts/1", "/actions/runs/900/attempts/1/jobs?per_page=100",
        "/actions/runs/900/artifacts?per_page=100",
        "/actions/artifacts/11/zip", "/actions/artifacts/13/zip",
        &format!("/git/ref/tags/{}", tag()), "/git/ref/heads/main",
        &format!("/compare/{SOURCE}...{MAIN}"),
    ]
    .iter()
    .map(|s| match *s {
        "USER" => "/users/donbeave".to_owned(),
        other => format!("{R}{other}"),
    })
    .collect();
    assert_eq!(calls, expected);
}

#[test]
fn the_metadata_binds_the_tag_run_the_annotation_and_the_qualification_runs() {
    let admission = helper(&AdmitWorld::new())
        .0
        .unwrap_or_else(|e| unreachable!("{e}"));
    let archive = |key: &str| sha(&AdmitWorld::new().zips[if key == "linux" { &11 } else { &13 }]);
    let target = |run: u64, artifact: u64, job: u64, key: &str| {
        json!({"producer": {"producer_run_id": run,
        "producer_run_attempt": 1, "payload_artifact_id": artifact, "payload_archive_sha256": archive(key)},
        "qualification_job_id": job})
    };
    let run = |path: &str, id: u64| {
        json!({"workflow_path": path, "workflow_ref": format!("{REPO}/{path}@refs/heads/main"),
        "workflow_sha": SOURCE, "run_id": id, "run_attempt": 1})
    };
    assert_eq!(
        admission.metadata,
        json!({"schema": 3, "repository": REPO, "version": "0.1.82", "source_sha": SOURCE, "tag": tag(),
            "source": {"workflow_path": SIGNER, "workflow_ref": format!("{REPO}/{SIGNER}@refs/tags/{}", tag()),
                       "workflow_sha": SOURCE, "run_id": 5000, "run_attempt": 1},
            "ci": run(".github/workflows/ci.yml", 600),
            "qualification": run(".github/workflows/source-qualification.yml", 700),
            "targets": {"linux": target(800, 11, 8101, "linux"), "darwin": target(900, 13, 9101, "darwin")}})
    );
    let triples: Vec<&str> = admission.payloads.iter().map(|p| p.0).collect();
    assert_eq!(
        triples,
        ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]
    );
    assert_eq!(admission.payloads[0].1, linux_payload());
    assert_eq!(admission.payloads[1].1, darwin_payload());
}

#[test]
fn every_broken_check_is_refused() {
    for (name, mutate) in all() {
        let mut world = AdmitWorld::new();
        mutate(&mut world);
        assert!(helper(&world).0.is_err(), "admitted: {name}");
    }
}

fn published(name: &str) -> (PathBuf, PathBuf, Res<()>) {
    let root = directory(name);
    let output = root.join("output");
    let temp = root.join("temp");
    fs::create_dir(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    let admission = helper(&AdmitWorld::new())
        .0
        .unwrap_or_else(|e| unreachable!("{e}"));
    let result = publish(&temp, &output, &admission);
    (temp, output, result)
}

#[test]
fn the_payloads_are_private_executables_next_to_the_metadata_and_the_output_names_the_source() {
    let (temp, output, result) = published("files");
    assert!(result.is_ok(), "{:?}", result.err());
    let files = tree(&temp);
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        [
            "source-preview-metadata.json",
            "velnor-actions-aarch64-apple-darwin",
            "velnor-actions-x86_64-unknown-linux-gnu"
        ]
    );
    assert_eq!(
        files["velnor-actions-x86_64-unknown-linux-gnu"],
        (linux_payload(), 0o500)
    );
    assert_eq!(
        files["velnor-actions-aarch64-apple-darwin"],
        (darwin_payload(), 0o500)
    );
    let metadata = helper(&AdmitWorld::new())
        .0
        .unwrap_or_else(|e| unreachable!("{e}"))
        .metadata;
    assert_eq!(
        files["source-preview-metadata.json"].0,
        canonical(&metadata)
    );
    assert_eq!(
        fs::read_to_string(output).unwrap_or_default(),
        format!("source_sha={SOURCE}\n")
    );
}

#[test]
fn nothing_is_replaced_and_a_failed_write_leaves_no_payload() {
    for (name, existing) in [
        ("payload", "velnor-actions-aarch64-apple-darwin"),
        ("metadata", "source-preview-metadata.json"),
    ] {
        let root = directory(&format!("exists-{name}"));
        let temp = root.join("temp");
        fs::create_dir(&temp).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(temp.join(existing), b"mine").unwrap_or_else(|e| unreachable!("{e}"));
        let admission = helper(&AdmitWorld::new())
            .0
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            publish(&temp, &root.join("output"), &admission).is_err(),
            "{name}"
        );
        assert_eq!(tree(&temp).keys().collect::<Vec<_>>(), [existing], "{name}");
        assert_eq!(fs::read(temp.join(existing)).unwrap_or_default(), b"mine");
        assert!(!root.join("output").exists(), "{name}");
    }
}
