// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_ci::cli::ImagePublishArgs;
use velnor_ci::image_candidate::hashing::sha256_hex;
use velnor_ci::image_candidate::record::INPUT_PATHS;
use velnor_ci::image_publish::checks::{ci_path, repository_path, tip_path};
use velnor_ci::image_publish::gate::{DEFAULT_EVENT, DEFAULT_WORKFLOW};
use velnor_ci::io::{Run, Transport, publish_with};
use zeroize::Zeroizing;

use super::gate::env as base_env;
use super::registry::{ACTOR, TOKEN};
use crate::image_candidate::fixture::{LAYER, build, check_for, plain_layer, spec, tar};
use crate::image_candidate::record::record;
use crate::support::fresh_dir;

pub(super) const PRODUCER_WORKFLOW: &str = "tailrocks/velnor-new/.github/workflows/ci.yml";
pub(super) const ARTIFACTS: &str =
    "repos/tailrocks/velnor-new/actions/runs/50/artifacts?per_page=100";

const GH: &str = r#"#!/bin/sh
FAKE='{FAKE}'
printf '%s\n' "$*" >> "$FAKE/gh.log"
if [ "$1" = "api" ]; then
  name=$(printf '%s' "$2" | tr '/?&=' '____')
  count=$(grep -c -F -x "api $2" "$FAKE/gh.log")
  if [ -f "$FAKE/api/$name.$count" ]; then cat "$FAKE/api/$name.$count"; exit 0; fi
  if [ -f "$FAKE/api/$name" ]; then cat "$FAKE/api/$name"; exit 0; fi
  exit 1
fi
if [ "$1" = "attestation" ]; then
  if [ -f "$FAKE/attestation.fail" ]; then echo "{TOKEN} leaked" >&2; exit 1; fi
  cat "$FAKE/attestation.json"
  exit 0
fi
exit 2
"#;

pub(super) struct Candidate {
    dir: velnor_test_support::ScratchDir,
    pub(super) head: String,
    pub(super) record: Value,
}

fn write(path: &Path, data: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap_or(path)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path, data).unwrap_or_else(|e| unreachable!("{e}"));
}

fn git(checkout: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(checkout)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn file_name(path: &str) -> String {
    path.replace(['/', '?', '&', '='], "_")
}

impl Candidate {
    pub(super) fn new(label: &str) -> Self {
        let dir = fresh_dir("velnor-ci-image-publish", label);
        let root = dir.join("root");
        for name in INPUT_PATHS {
            write(&root.join(name), format!("content of {name}\n"));
        }
        write(
            &root.join(".velnor/runner.lock"),
            "buildx_version = \"0.38.0\"\n",
        );
        git(&root, &["init", "-q"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
        let head = git(&root, &["rev-parse", "HEAD"]);
        let second = tar(&[("second", b"second-layer")]);
        let mut layers = spec(vec![(plain_layer(), LAYER), (second, LAYER)]);
        layers.revision = Some(head.clone());
        let archive = build(&layers).archive();
        let mut value = record();
        let inputs: serde_json::Map<String, Value> = INPUT_PATHS
            .iter()
            .map(|name| {
                let bytes = fs::read(root.join(name)).unwrap_or_default();
                ((*name).to_owned(), json!(sha256_hex(&bytes)))
            })
            .collect();
        value["source"] = json!({"commit": head, "ref": "refs/heads/main", "inputs": inputs});
        value["build"]["arguments"] = json!([format!("VELNOR_SOURCE_SHA={head}")]);
        value["workflow"] = json!({
            "repository": "tailrocks/velnor-new", "repository_id": 1_390_620_900_u64,
            "event": "push", "workflow_ref": format!("{PRODUCER_WORKFLOW}@refs/heads/main"),
            "authority_commit": head, "run_id": 50, "run_attempt": 1,
        });
        value["archive"] = json!({"sha256": sha256_hex(&archive), "size_bytes": archive.len()});
        value["oci"] = check_for(&archive, &head).unwrap_or_else(|e| unreachable!("{e}"));
        write(&dir.join("out/candidate.oci.tar"), &archive);
        write(&dir.join("out/candidate.json"), value.to_string());
        let gh = dir.join("bin/gh");
        write(
            &gh,
            GH.replace("{FAKE}", &dir.display().to_string())
                .replace("{TOKEN}", TOKEN),
        );
        fs::set_permissions(&gh, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let candidate = Self {
            dir,
            head,
            record: value,
        };
        candidate.defaults();
        candidate
    }

    fn defaults(&self) {
        self.api(
            &repository_path(),
            &json!({"full_name": "tailrocks/velnor-new", "id": 1_390_620_900_u64}),
        );
        self.api(&tip_path(), &json!({"sha": self.head}));
        self.api(
            ARTIFACTS,
            &json!({"artifacts": [{"name": "velnor-job-image-candidate-1", "id": 99,
                "expired": false,
                "workflow_run": {"id": 50, "head_sha": self.head, "head_branch": "main"}}]}),
        );
        self.api(
            &ci_path(&self.head),
            &json!({"workflow_runs": [{
                "id": 50, "run_number": 5, "run_attempt": 1, "status": "completed",
                "conclusion": "success", "path": ".github/workflows/ci.yml", "event": "push",
                "head_branch": "main", "head_sha": self.head,
                "head_repository": {"full_name": "tailrocks/velnor-new"}}]}),
        );
        write(
            &self.dir.join("attestation.json"),
            json!([{"verificationResult": {"signature": {"certificate": {
                "sourceRepositoryIdentifier": "1390620900",
                "githubWorkflowRepository": "tailrocks/velnor-new",
                "githubWorkflowSHA": self.head, "githubWorkflowRef": "refs/heads/main",
                "githubWorkflowTrigger": "push",
                "sourceRepositoryRef": "refs/heads/main",
                "sourceRepositoryDigest": self.head}}}}])
            .to_string(),
        );
    }

    pub(super) fn api(&self, path: &str, value: &Value) {
        write(
            &self.dir.join("api").join(file_name(path)),
            value.to_string(),
        );
    }

    pub(super) fn api_call(&self, path: &str, call: usize, value: &Value) {
        let name = format!("{}.{call}", file_name(path));
        write(&self.dir.join("api").join(name), value.to_string());
    }

    pub(super) fn fail_attestation(&self) {
        write(&self.dir.join("attestation.fail"), "");
    }

    pub(super) fn gh_log(&self) -> String {
        fs::read_to_string(self.dir.join("gh.log")).unwrap_or_default()
    }

    pub(super) fn drop_checkout(&self) {
        fs::remove_dir_all(self.dir.join("root")).unwrap_or_else(|e| unreachable!("{e}"));
    }

    pub(super) fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub(super) fn env(&self) -> HashMap<String, String> {
        let mut vars = base_env();
        for name in ["GITHUB_SHA", "GITHUB_WORKFLOW_SHA"] {
            vars.insert(name.to_owned(), self.head.clone());
        }
        vars.insert("GITHUB_ACTOR".to_owned(), ACTOR.to_owned());
        vars
    }

    pub(super) fn args(&self) -> ImagePublishArgs {
        ImagePublishArgs {
            archive: self.path("out/candidate.oci.tar"),
            record: self.path("out/candidate.json"),
            result: self.path("evidence/publication.json"),
            workflow: DEFAULT_WORKFLOW.to_owned(),
            event: DEFAULT_EVENT.to_owned(),
        }
    }

    pub(super) fn publish(
        &self,
        transport: &dyn Transport,
        vars: HashMap<String, String>,
    ) -> (Result<String, String>, Vec<Duration>) {
        let pauses = Mutex::new(Vec::new());
        let gh = self.path("bin/gh");
        let record = |duration: Duration| {
            pauses
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(duration);
        };
        let result = publish_with(
            &self.args(),
            &Run {
                vars,
                token: Zeroizing::new(TOKEN.to_owned()),
                gh: &gh,
                transport,
                pause: &record,
            },
        );
        let seen = pauses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        (result.map_err(|e| e.to_string()), seen)
    }

    pub(super) fn binary(
        &self,
        vars: &HashMap<String, String>,
        extra: &[&str],
    ) -> (bool, String, String) {
        let path = format!(
            "{}:{}",
            self.path("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let args = self.args();
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .env_clear()
            .args(["image", "publish", "--archive"])
            .arg(&args.archive)
            .arg("--record")
            .arg(&args.record)
            .arg("--result")
            .arg(&args.result)
            .args(extra)
            .envs(vars)
            .env("PATH", path)
            .env("GH_TOKEN", TOKEN)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}
