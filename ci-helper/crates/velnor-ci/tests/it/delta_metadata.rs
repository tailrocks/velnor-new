// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes};
use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::delta::success;
use crate::delta_source::{
    cargo, delta, fixture, helper, helper_at, next_push, prepare, save_inputs,
};
use crate::io::Repo;

const SOURCE_A: &str = "fn main() {}\n";
const SOURCE_B: &str = "compile_error!(\"changed source must compile\");\nfn main() {}\n";
const FUTURE_SECONDS: u64 = 4_102_444_800;

fn bound(identity: &str, body: &str) -> String {
    let checksum = velnor_rust::sha256_hex(format!("{identity}\n{body}").as_bytes());
    format!("velnor-inputs-v1\t{identity}\t{checksum}\n{body}")
}

#[test]
fn checksum_valid_snapshots_must_match_the_built_checkout_records() {
    let (repo, expected) = fixture("delta-current-records");
    let original = repo.mtime("src/main.rs");
    save_inputs(&repo);
    let path = repo.root().join("target/.velnor/inputs.tsv");
    let first = fs::read_to_string(&path).unwrap_or_else(|error| unreachable!("{error}"));
    let (_, first_body) = first.split_once('\n').unwrap_or_else(|| unreachable!());
    repo.write_unstaged("src/main.rs", SOURCE_B);
    repo.set_mtime("src/main.rs", original);
    save_inputs(&repo);
    let changed = fs::read_to_string(&path).unwrap_or_else(|error| unreachable!("{error}"));
    let (_, changed_body) = changed.split_once('\n').unwrap_or_else(|| unreachable!());
    repo.write_unstaged("src/main.rs", SOURCE_A);
    repo.set_mtime("src/main.rs", original);
    let row = first_body
        .lines()
        .find(|line| line.starts_with("src/main.rs\t"))
        .unwrap_or_else(|| unreachable!());
    let missing = first_body
        .lines()
        .filter(|line| !line.starts_with("src/main.rs\t"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let duplicate = format!("{first_body}{row}\n");
    let extra = format!(
        "{first_body}{}\n",
        row.replacen("src/main.rs", "extra.rs", 1)
    );
    let mut decisions = Vec::new();
    for body in [&duplicate, &missing, &extra, changed_body] {
        fs::write(&path, bound(&expected, body)).unwrap_or_else(|error| unreachable!("{error}"));
        decisions.push(delta(&repo, "stage", Some(&expected)));
    }
    repo.write_unstaged("src/main.rs", SOURCE_B);
    repo.set_mtime("src/main.rs", original);
    let output = next_push(&repo);
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "changed source was falsely fresh: {log}"
    );
    assert!(log.contains("changed source must compile"), "{log}");
    assert!(
        decisions.iter().all(|decision| decision == "save=false\n"),
        "{decisions:?}"
    );
}

#[test]
fn scoped_snapshots_can_stage_a_nested_target_with_other_tracked_workspaces() {
    let repo = Repo::new("delta-nested-records");
    repo.write_all(&[
        ("workspace/src.rs", "nested source"),
        ("other/src.rs", "other source"),
    ]);
    let expected = prepare(&repo);
    let invoke = |verb| {
        helper(
            &repo,
            &[
                "delta",
                verb,
                "--target-dir",
                "workspace/target",
                "--staging-dir",
                "delta",
                "--package",
                "app",
            ],
            Some(&expected),
        )
    };
    success(&invoke("apply").0);
    repo.write_unstaged("workspace/target/debug/artifact", "built nested source");
    repo.write_unstaged("velnor-run.log", "Compiling app v0.1.0\n");
    success(
        &helper_at(
            &repo,
            &["inputs", "save"],
            None,
            &repo.root().join("workspace"),
        )
        .0,
    );
    let (output, values) = invoke("stage");
    success(&output);
    assert_eq!(values, "save=true\n");
}

fn future_time(path: &Path) {
    File::open(path)
        .and_then(|file| {
            file.set_times(
                FileTimes::new()
                    .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(FUTURE_SECONDS)),
            )
        })
        .unwrap_or_else(|error| unreachable!("{error}"));
}

fn future_tree(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in fs::read_dir(&path).unwrap_or_else(|error| unreachable!("{error}")) {
                pending.push(entry.unwrap_or_else(|error| unreachable!("{error}")).path());
            }
        }
        future_time(&path);
    }
}

#[test]
fn build_outputs_with_future_modification_times_decline_delta_saving() {
    let (repo, expected) = fixture("delta-future-stage");
    save_inputs(&repo);
    future_tree(&repo.root().join("target"));
    assert_eq!(delta(&repo, "stage", Some(&expected)), "save=false\n");
    assert!(!repo.root().join("delta").exists());
}

#[test]
fn future_metadata_in_a_delta_cannot_make_changed_source_fresh() {
    future_restore("delta");
}

#[test]
fn future_metadata_in_a_base_cannot_make_changed_source_fresh() {
    future_restore("base");
}

fn future_restore(layer: &str) {
    let (repo, expected) = fixture("delta-future-restore");
    save_inputs(&repo);
    if layer == "delta" {
        assert_eq!(delta(&repo, "stage", Some(&expected)), "save=true\n");
        let manifest = repo.root().join("delta/manifest.json");
        let mut metadata: serde_json::Value = serde_json::from_slice(
            &fs::read(&manifest).unwrap_or_else(|error| unreachable!("{error}")),
        )
        .unwrap_or_else(|error| unreachable!("{error}"));
        for (path, node) in metadata["nodes"]
            .as_object_mut()
            .unwrap_or_else(|| unreachable!())
        {
            node["modified"] = serde_json::json!(FUTURE_SECONDS * 1_000_000_000);
            future_time(&repo.root().join("delta/files").join(path));
        }
        fs::write(
            manifest,
            serde_json::to_vec(&metadata).unwrap_or_else(|error| unreachable!("{error}")),
        )
        .unwrap_or_else(|error| unreachable!("{error}"));
        fs::remove_dir_all(repo.root().join("target"))
            .unwrap_or_else(|error| unreachable!("{error}"));
    } else {
        future_tree(&repo.root().join("target"));
    }
    repo.write_unstaged("src/main.rs", SOURCE_B);
    let (output, _) = helper(
        &repo,
        &[
            "delta",
            "apply",
            "--target-dir",
            "target",
            "--staging-dir",
            "delta",
        ],
        None,
    );
    success(&output);
    success(&helper(&repo, &["inputs", "restore"], None).0);
    let compiled = cargo(&repo);
    let log = String::from_utf8_lossy(&compiled.stderr);
    assert!(
        !compiled.status.success(),
        "{layer} made changed source falsely fresh: {log}"
    );
    assert!(log.contains("changed source must compile"), "{log}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("building cold"),
        "{layer}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
