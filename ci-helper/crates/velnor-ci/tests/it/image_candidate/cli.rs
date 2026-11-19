// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};
use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::image_candidate::record::INPUT_PATHS;

use super::fixture::{LAYER, LAYER_GZ, build, check, gzip, plain_layer, spec};
use super::record::record;
use crate::support::fresh_dir;

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct Candidate {
    dir: PathBuf,
}

impl Candidate {
    fn new(label: &str, archive: &[u8]) -> Self {
        let dir = fresh_dir("velnor-ci-image-candidate", label);
        let root = dir.join("root");
        for name in INPUT_PATHS {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap_or(&root))
                .unwrap_or_else(|e| unreachable!("{e}"));
            fs::write(&path, format!("content of {name}\n"))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        fs::write(
            root.join(".velnor/runner.lock"),
            "buildx_version = \"0.38.0\"\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(dir.join("candidate.oci.tar"), archive).unwrap_or_else(|e| unreachable!("{e}"));
        let candidate = Self { dir };
        let mut value = record();
        value["source"]["commit"] = json!(SHA);
        for name in INPUT_PATHS {
            let bytes = fs::read(candidate.dir.join("root").join(name)).unwrap_or_default();
            let digest = sha256_hex(&bytes);
            value["source"]["inputs"][name] = json!(digest);
        }
        value["archive"] = json!({"sha256": sha256_hex(archive), "size_bytes": archive.len()});
        if let Ok(graph) = check(archive) {
            value["oci"] = graph;
        }
        candidate.write_record(&value);
        candidate
    }

    fn write_record(&self, value: &Value) {
        fs::write(self.dir.join("candidate.json"), value.to_string())
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn edit(&self, change: impl FnOnce(&mut Value)) {
        let raw = fs::read(self.dir.join("candidate.json")).unwrap_or_else(|e| unreachable!("{e}"));
        let mut value: Value = serde_json::from_slice(&raw).unwrap_or_else(|e| unreachable!("{e}"));
        change(&mut value);
        self.write_record(&value);
    }

    fn run(&self, env: &[(&str, Option<&str>)]) -> (bool, String, String) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .env_clear()
            .args(["image", "verify-candidate", "--root"]);
        command.arg(self.dir.join("root"));
        command
            .arg("--archive")
            .arg(self.dir.join("candidate.oci.tar"));
        command.arg("--record").arg(self.dir.join("candidate.json"));
        let mut vars = vec![
            ("GITHUB_SHA", SHA.to_owned()),
            ("GITHUB_REF", "refs/heads/candidate".to_owned()),
            ("GITHUB_REPOSITORY", "tailrocks/velnor-new".to_owned()),
            ("GITHUB_REPOSITORY_ID", "1".to_owned()),
            ("GITHUB_EVENT_NAME", "push".to_owned()),
            (
                "GITHUB_WORKFLOW_REF",
                "tailrocks/velnor-new/.github/workflows/release.yml@refs/heads/main".to_owned(),
            ),
            ("GITHUB_WORKFLOW_SHA", "c".repeat(40)),
            ("GITHUB_RUN_ID", "10".to_owned()),
            ("GITHUB_RUN_ATTEMPT", "2".to_owned()),
        ];
        for (name, value) in env {
            vars.retain(|(known, _)| known != name);
            if let Some(value) = value {
                vars.push((name, (*value).to_owned()));
            }
        }
        command.envs(vars);
        let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Candidate {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.dir).ok();
    }
}

fn archive() -> Vec<u8> {
    let layer = plain_layer();
    build(&spec(vec![(gzip(&layer), LAYER_GZ), (layer, LAYER)])).archive()
}

fn refusal(candidate: &Candidate, env: &[(&str, Option<&str>)]) -> String {
    let (ok, _, stderr) = candidate.run(env);
    assert!(!ok, "accepted: {stderr}");
    stderr.trim().trim_start_matches("velnor-ci: ").to_owned()
}

#[test]
fn an_attested_candidate_whose_record_matches_everything_passes() {
    let candidate = Candidate::new("pass", &archive());
    let (ok, stdout, stderr) = candidate.run(&[]);
    assert!(ok, "{stderr}");
    assert_eq!(
        stdout,
        "PASS: Candidate record has the same source inputs and OCI archive identities.\n"
    );
}

#[test]
fn an_archive_whose_bytes_changed_after_the_record_was_written_is_refused() {
    let candidate = Candidate::new("digest", &archive());
    let mut changed = archive();
    changed.extend_from_slice(&[0; 512]);
    fs::write(candidate.path("candidate.oci.tar"), changed).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        refusal(&candidate, &[]),
        "Candidate archive is not the same as the archive in its record."
    );
}

#[test]
fn a_record_whose_graph_is_not_the_archive_graph_is_refused() {
    let candidate = Candidate::new("graph", &archive());
    candidate.edit(|value| {
        value["oci"]["layer_digests"] = json!([format!("sha256:{}", "0".repeat(64))]);
    });
    assert_eq!(
        refusal(&candidate, &[]),
        "OCI graph is not the same as the graph in its candidate record."
    );
}

#[test]
fn a_source_input_changed_after_the_candidate_was_built_is_refused() {
    let candidate = Candidate::new("input", &archive());
    for name in INPUT_PATHS {
        let path = candidate.path("root").join(name);
        let original = fs::read(&path).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(&path, "changed\n").unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            refusal(&candidate, &[]),
            "Build input bytes changed after candidate creation.",
            "{name}"
        );
        fs::remove_file(&path).unwrap_or_else(|e| unreachable!("{e}"));
        assert!(refusal(&candidate, &[]).contains(name), "{name} missing");
        fs::write(&path, original).unwrap_or_else(|e| unreachable!("{e}"));
    }
    assert!(candidate.run(&[]).0);
}

#[test]
fn another_commit_ref_or_workflow_than_the_recorded_ones_is_refused() {
    let candidate = Candidate::new("identity", &archive());
    let source = "Candidate source identity is not the same as the current GitHub source identity.";
    let other_sha = "b".repeat(40);
    assert_eq!(
        refusal(&candidate, &[("GITHUB_SHA", Some(&other_sha))]),
        source
    );
    assert_eq!(refusal(&candidate, &[("GITHUB_SHA", None)]), source);
    assert_eq!(
        refusal(&candidate, &[("GITHUB_REF", Some("refs/heads/other"))]),
        source
    );
    let workflow =
        "Candidate workflow identity is not the same as the current GitHub workflow identity.";
    for (name, value) in [
        ("GITHUB_RUN_ID", "11"),
        ("GITHUB_RUN_ATTEMPT", "3"),
        ("GITHUB_EVENT_NAME", "schedule"),
        ("GITHUB_REPOSITORY_ID", "2"),
    ] {
        assert_eq!(
            refusal(&candidate, &[(name, Some(value))]),
            workflow,
            "{name}"
        );
    }
    assert_eq!(
        refusal(&candidate, &[("GITHUB_RUN_ID", None)]),
        "GitHub run_id is incorrect."
    );
}

#[test]
fn another_buildx_than_the_locked_one_is_refused() {
    let candidate = Candidate::new("buildx", &archive());
    fs::write(
        candidate.path("root/.velnor/runner.lock"),
        "buildx_version = \"0.40.0\"\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let lock = sha256_hex(b"buildx_version = \"0.40.0\"\n");
    candidate.edit(|value| value["source"]["inputs"][".velnor/runner.lock"] = json!(lock));
    assert_eq!(
        refusal(&candidate, &[]),
        "Candidate Buildx identity is not the same as the value in .velnor/runner.lock."
    );
}

#[test]
fn a_record_or_archive_that_is_a_symlink_or_not_a_file_is_refused() {
    let candidate = Candidate::new("links", &archive());
    let real = candidate.path("real.json");
    fs::rename(candidate.path("candidate.json"), &real).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&real, candidate.path("candidate.json")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        refusal(&candidate, &[]),
        "Candidate record must be a regular file."
    );
    fs::remove_file(candidate.path("candidate.json")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::rename(&real, candidate.path("candidate.json")).unwrap_or_else(|e| unreachable!("{e}"));
    let archive_real = candidate.path("real.tar");
    fs::rename(candidate.path("candidate.oci.tar"), &archive_real)
        .unwrap_or_else(|e| unreachable!("{e}"));
    symlink(&archive_real, candidate.path("candidate.oci.tar"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        refusal(&candidate, &[]),
        "OCI archive must be a regular file."
    );
}

#[test]
fn an_oversized_or_malformed_record_is_refused() {
    let candidate = Candidate::new("record", &archive());
    fs::write(candidate.path("candidate.json"), vec![b' '; (1 << 20) + 1])
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        refusal(&candidate, &[]),
        "Candidate record exceeds its size limit."
    );
    fs::write(candidate.path("candidate.json"), b"{not json")
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        refusal(&candidate, &[]),
        "candidate record JSON has an incorrect format."
    );
    fs::write(
        candidate.path("candidate.json"),
        br#"{"schema":"a","schema":"b"}"#,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refusal(&candidate, &[]), "Duplicate JSON key: schema");
}

#[test]
fn a_compressed_outer_archive_is_refused_before_it_is_parsed() {
    let candidate = Candidate::new("compressed", &gzip(&archive()));
    assert_eq!(
        refusal(&candidate, &[]),
        "OCI archive must use an uncompressed tar format."
    );
}
