// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate as velnor_actions_orchestrator;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    fs,
    path::Path,
    process::Command,
};
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_controller::{
        CrateReleaseAction, CrateReleaseRepairWrite,
        prepare_crate_release as raw_prepare_crate_release, reconcile_crate_release,
        repair_crate_release, verify_crate_release_admission,
    },
    crate_release_github::{GitHubAdmissionRead, GitHubAdmissionRequest},
    crate_release_registry::{CrateRegistryRead, CrateRegistryResponse},
};

fn fixture_no_baseline() -> crate::crate_release_semver::SemverEvidence {
    crate::crate_release_semver::SemverEvidence::NoBaseline {
        checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829".into(),
    }
}

fn fixture_semver_map(
    receipt: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<
    std::collections::BTreeMap<String, crate::crate_release_semver::SemverEvidence>,
    OrchestratorError,
> {
    let checker = velnor_actions_mise::catalog::qualified_cargo_semver_checks("ubuntu-24.04")
        .ok_or_else(|| OrchestratorError::Contract {
            problem: "fixture".into(),
        })?;
    Ok(receipt
        .packages
        .iter()
        .map(|p| {
            (
                p.source.name.clone(),
                crate::crate_release_semver::SemverEvidence::NoBaseline {
                    checker_sha256: checker.executable_sha256.into(),
                },
            )
        })
        .collect())
}

fn prepare_crate_release<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
) -> Result<crate::crate_release_controller::CrateReleaseAdmissionReceipt, OrchestratorError> {
    let mut receipt = raw_prepare_crate_release(intent, request, github, registry, source)?;
    receipt.semver_evidence = fixture_semver_map(&receipt)?;
    Ok(receipt)
}

#[test]
fn reconciliation_waits_only_for_visibility() -> Result<(), Box<dyn std::error::Error>> {
    use std::cell::Cell;
    use velnor_actions_orchestrator::crate_release_controller::wait_for_reconciliation_visibility;
    let calls = Cell::new(0);
    let waits = Cell::new(0);
    wait_for_reconciliation_visibility(
        3,
        || {
            calls.set(calls.get() + 1);
            Ok(calls.get() == 3)
        },
        || waits.set(waits.get() + 1),
    )?;
    assert_eq!(waits.get(), 2);
    assert!(wait_for_reconciliation_visibility(2, || Ok(false), || {}).is_err());
    let calls = Cell::new(0);
    assert!(
        wait_for_reconciliation_visibility(
            3,
            || {
                calls.set(calls.get() + 1);
                Err(OrchestratorError::Contract {
                    problem: "denied".into(),
                })
            },
            || {}
        )
        .is_err()
    );
    assert_eq!(calls.get(), 1);
    Ok(())
}

type MetadataWriteHook = Box<dyn Fn(u32)>;
type RefCreatedHook = Box<dyn Fn(&str)>;

struct Github {
    sha: String,
    policy_sha: String,
    approved: Cell<bool>,
    tag_exists: Cell<bool>,
    tag_wrong: Cell<bool>,
    owner_match: Cell<bool>,
    release_variant: Cell<u8>,
    write_error: Cell<u8>,
    writes: Cell<u32>,
    tool_archive: RefCell<Vec<u8>>,
    mutate_after_download: Cell<u8>,
    artifact_receipt: Cell<bool>,
    before_metadata_write: RefCell<Option<MetadataWriteHook>>,
}
impl Github {
    fn tool_metadata(&self) -> Value {
        let bytes = self.tool_archive.borrow();
        let mut digest = String::from("sha256:");
        for byte in Sha256::digest(&*bytes) {
            digest.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
            digest.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
        }
        json!({"id":10,"name":if self.artifact_receipt.get() { "velnor-release-receipt-12-1" } else { "velnor-release-rust-12-1" },"digest":digest,"size_in_bytes":bytes.len(),"expired":false,"workflow_run":{"id":12,"repository_id":22,"head_repository_id":22,"head_sha":self.policy_sha}})
    }
}
impl velnor_actions_orchestrator::crate_release_tool_download::ToolDownloadTransport for Github {
    fn github(
        &self,
        _: &str,
        _: u64,
        _: &str,
        _: std::time::Instant,
    ) -> Result<
        velnor_actions_orchestrator::crate_release_tool_download::ToolDownloadResponse,
        OrchestratorError,
    > {
        Ok(
            velnor_actions_orchestrator::crate_release_tool_download::ToolDownloadResponse {
                status: 302,
                location: Some("https://fixture.blob.core.windows.net/a".into()),
                content_length: None,
                content_encoding: None,
                body: Box::new(std::io::Cursor::new(Vec::new())),
            },
        )
    }
    fn storage(
        &self,
        _: &str,
        _: std::time::Instant,
    ) -> Result<
        velnor_actions_orchestrator::crate_release_tool_download::ToolDownloadResponse,
        OrchestratorError,
    > {
        let bytes = self.tool_archive.borrow().clone();
        match self.mutate_after_download.get() {
            1 => self.approved.set(false),
            2 => self.tool_archive.borrow_mut().push(0),
            _ => {}
        }
        Ok(
            velnor_actions_orchestrator::crate_release_tool_download::ToolDownloadResponse {
                status: 200,
                location: None,
                content_length: Some(bytes.len() as u64),
                content_encoding: None,
                body: Box::new(std::io::Cursor::new(bytes)),
            },
        )
    }
}
impl GitHubAdmissionRead for Github {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let sha = &self.sha;
        let policy_sha = &self.policy_sha;
        let checked_sha = if route.contains(policy_sha) {
            policy_sha
        } else {
            sha
        };
        let data = if route.ends_with("/actions/runs/12/artifacts?per_page=100") {
            json!({"total_count":1,"artifacts":[self.tool_metadata()]})
        } else if route.ends_with("/actions/artifacts/10") {
            self.tool_metadata()
        } else if route.ends_with("/rulesets?targets=tag&includes_parents=true&per_page=100") {
            json!([{"id":1}])
        } else if route.ends_with("/rulesets/1") {
            json!({"id":1,"target":"tag","enforcement":"active","bypass_actors":[],"conditions":{"ref_name":{"include":["~ALL"],"exclude":[]}},"rules":[{"type":"update","parameters":{"update_allows_fetch_and_merge":false}},{"type":"deletion"}]})
        } else if route == "repos/example/demo" {
            json!({"id":22,"full_name":"example/demo","default_branch":"main"})
        } else if route.ends_with("/actions/runs/12/approvals") {
            if self.approved.get() {
                json!([{"state":"approved","user":{"type":"User","id":139_017},"environments":[{"id":9,"name":"release"}],"comment":format!("I reviewed the complete release policy at {policy_sha} and authorize source {sha} for run 12.")}])
            } else {
                json!([])
            }
        } else if route.ends_with("/actions/runs/12") {
            json!({"id":12,"run_attempt":1,"head_sha":policy_sha,"head_branch":"main","event":"workflow_dispatch","path":".github/workflows/release.yml","repository":{"id":22,"full_name":"example/demo"},"head_repository":{"id":22,"full_name":"example/demo"}})
        } else if route.contains("/compare/") {
            json!({"status":"ahead"})
        } else if route.ends_with("/environments/release") {
            json!({"id":9,"name":"release","can_admins_bypass":false,"deployment_branch_policy":{"custom_branch_policies":true},"protection_rules":[{"type":"required_reviewers","reviewers":[{"type":"User","reviewer":{"id":139_017}}]}]})
        } else if route.contains("deployment-branch-policies") {
            json!({"total_count":1,"branch_policies":[{"type":"branch","name":"main"}]})
        } else if route.contains("check-runs?") {
            json!({"total_count":1,"check_runs":[{"id":1,"name":"Required","head_sha":checked_sha,"check_suite":{"id":123},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"}]})
        } else if route.contains("/actions/runs?") {
            json!({"total_count":1,"workflow_runs":[{"check_suite_id":123,"head_sha":checked_sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]})
        } else if route.ends_with("/git/ref/heads/main") {
            json!({"ref":"refs/heads/main","object":{"type":"commit","sha":sha}})
        } else if route == "users/owner" {
            json!({"id":if self.owner_match.get() {139_017} else {999},"type":"User","login":"owner"})
        } else {
            return Err(OrchestratorError::Contract {
                problem: format!("unexpected route {route}"),
            });
        };
        Ok(data)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        if !self.tag_exists.get() {
            return Ok(None);
        }
        if route.contains("/releases/tags/") {
            if self.release_variant.get() == 4 {
                return Ok(None);
            }
            let tag = if self.release_variant.get() == 2 {
                "wrong-tag"
            } else {
                "demo-v0.1.1"
            };
            let url = if self.release_variant.get() == 3 {
                "https://example.com/wrong"
            } else {
                "https://github.com/example/demo/releases/tag/demo-v0.1.1"
            };
            let id = match self.release_variant.get() {
                5 => json!(null),
                6 => json!(0),
                7 => json!("12"),
                8 => json!(-1),
                9 => json!(18_446_744_073_709_551_616.0_f64),
                _ => json!(12),
            };
            return Ok(Some(
                json!({"id":id,"tag_name":tag,"draft":self.release_variant.get() == 1,"html_url":url}),
            ));
        }
        Ok(Some(
            json!({"ref":"refs/tags/demo-v0.1.1","object":{"type":"commit","sha":if self.tag_wrong.get() {"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"} else {&self.sha}}}),
        ))
    }
}
impl CrateReleaseRepairWrite for Github {
    fn create_tag(
        &self,
        _repository: &str,
        _tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        assert_eq!(source_sha, self.sha);
        if let Some(hook) = self.before_metadata_write.borrow().as_ref() {
            hook(1);
        }
        self.writes.set(self.writes.get() + 1);
        if self.write_error.get() == 1 {
            return Err(OrchestratorError::Contract {
                problem: "ambiguous tag write".into(),
            });
        }
        self.tag_exists.set(true);
        if self.write_error.get() == 3 {
            return Err(OrchestratorError::Contract {
                problem: "tag timeout after write".into(),
            });
        }
        Ok(())
    }
    fn create_release(
        &self,
        _repository: &str,
        _tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        assert_eq!(source_sha, self.sha);
        if let Some(hook) = self.before_metadata_write.borrow().as_ref() {
            hook(3);
        }
        self.writes.set(self.writes.get() + 1);
        if self.write_error.get() == 2 {
            return Err(OrchestratorError::Contract {
                problem: "ambiguous release write".into(),
            });
        }
        self.release_variant.set(0);
        Ok(())
    }
}
struct NewRegistry {
    base: Registry,
    visible: Cell<bool>,
    owner_changed: Cell<bool>,
}
impl CrateRegistryRead for NewRegistry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        if !self.visible.get() && path.starts_with("/api/v1/crates/demo") {
            return Ok(CrateRegistryResponse {
                status: 404,
                body: vec![],
            });
        }
        if self.visible.get() && self.owner_changed.get() && path == "/api/v1/crates/demo/owners" {
            return Ok(CrateRegistryResponse {
                status: 200,
                body: br#"{"users":[{"id":370533,"login":"owner","kind":"user"}]}"#.to_vec(),
            });
        }
        if self.visible.get() && path == "/api/v1/crates/demo" {
            return Ok(CrateRegistryResponse { status: 200, body: json!({"crate":{"id":"demo","name":"demo","versions":[2]},"versions":[{"id":2,"crate":"demo","num":"0.1.1","yanked":false}]}).to_string().into_bytes() });
        }
        self.base.get(path)
    }
}

struct Registry {
    absent: Cell<bool>,
    inconsistent: Cell<bool>,
    archive: RefCell<Vec<u8>>,
}
impl CrateRegistryRead for Registry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        if path == "/api/v1/crates/demo/owners" {
            return Ok(CrateRegistryResponse {
                status: 200,
                body: br#"{"users":[{"id":370532,"login":"owner","kind":"user"}]}"#.to_vec(),
            });
        }
        if path == "/api/v1/crates/demo" {
            let mut versions = vec![json!({"id":1,"crate":"demo","num":"0.1.0","yanked":false})];
            if !self.absent.get() && !self.inconsistent.get() {
                versions.push(json!({"id":2,"crate":"demo","num":"0.1.1","yanked":false}));
            }
            let ids: Vec<_> = versions
                .iter()
                .map(|version| version["id"].clone())
                .collect();
            return Ok(CrateRegistryResponse {
                status: 200,
                body:
                    json!({"crate":{"id":"demo","name":"demo","versions":ids},"versions":versions})
                        .to_string()
                        .into_bytes(),
            });
        }
        if path == "/api/v1/crates/demo/0.1.1" {
            if self.absent.get() {
                return Ok(CrateRegistryResponse {
                    status: 404,
                    body: Vec::new(),
                });
            }
            let digest = Sha256::digest(&*self.archive.borrow());
            let mut checksum = String::new();
            for byte in digest {
                checksum.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                checksum.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
            }
            return Ok(CrateRegistryResponse { status:200, body:json!({"version":{"id":2,"crate":"demo","num":"0.1.1","checksum":checksum,"yanked":false,"features":{"demo-feature":[]}}}).to_string().into_bytes() });
        }
        if path == "/crates/demo/demo-0.1.1.crate" {
            return Ok(CrateRegistryResponse {
                status: 200,
                body: self.archive.borrow().clone(),
            });
        }
        Err(OrchestratorError::Contract {
            problem: "unexpected registry route".to_owned(),
        })
    }
}
fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let result = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args(args)
        .output()?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

fn make_fixture(
    root: &Path,
) -> Result<
    (CrateReleaseIntent, GitHubAdmissionRequest, Github, Registry),
    Box<dyn std::error::Error>,
> {
    git(root, &["init", "-q"])?;
    git(root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(root, &["config", "user.email", "alexey@zhokhov.com"])?;
    git(root, &["config", "commit.gpgsign", "false"])?;
    git(root, &["config", "core.hooksPath", "/dev/null"])?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn demo() {}")?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.1\"\nedition = \"2021\"\ndescription = \"fixture\"\nlicense = \"MIT\"\n[features]\ndemo-feature = []\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.1\"\n",
    )?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "fixture",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    let intent: CrateReleaseIntent = serde_json::from_value(
        json!({"schema":1,"repository":"example/demo","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"rust_version":"1.97.1","generator_version":"0.1.72","generator_source_sha":"f1041f322c54cd7edfdef06afd307799eeaa801a","generator_manifest_sha256":"e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52","release_plz_version":"0.4.3","runner":"ubuntu-24.04","authentication":"trusted-publishing","manifest_path":"Cargo.toml","registry":"crates-io","packages":["demo"],"publish_features":{"demo":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":{"package":"demo","version":"0.1.1","source_sha":sha}}),
    )?;
    let request = GitHubAdmissionRequest {
        repository: "example/demo".into(),
        default_branch: "main".into(),
        environment: "release".into(),
        trusted_reviewer_id: 139_017,
        workflow_path: ".github/workflows/release.yml".into(),
        required_workflow_path: ".github/workflows/ci.yml".into(),
        workflow_sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        run_id: 12,
        run_attempt: 1,
        event: "workflow_dispatch".into(),
        source_sha: sha.clone(),
        recovery_sha: Some(sha.clone()),
    };
    let github = Github {
        sha,
        policy_sha: request.workflow_sha.clone(),
        approved: Cell::new(false),
        tag_exists: Cell::new(false),
        tag_wrong: Cell::new(false),
        owner_match: Cell::new(true),
        release_variant: Cell::new(0),
        write_error: Cell::new(0),
        writes: Cell::new(0),
        tool_archive: RefCell::new(Vec::new()),
        mutate_after_download: Cell::new(0),
        artifact_receipt: Cell::new(false),
        before_metadata_write: RefCell::new(None),
    };
    let registry = Registry {
        absent: Cell::new(true),
        inconsistent: Cell::new(false),
        archive: RefCell::new(Vec::new()),
    };
    Ok((intent, request, github, registry))
}

fn fixture_archive(root: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let target = tempfile::tempdir()?;
    let which = Command::new("mise")
        .current_dir(target.path())
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "which",
            "cargo",
            "--tool",
            "rust@1.97.1",
        ])
        .output()?;
    if !which.status.success() {
        return Err("cannot resolve pinned cargo".into());
    }
    let cargo = String::from_utf8(which.stdout)?.trim().to_owned();
    let package = Command::new("mise")
        .current_dir(root)
        .env("MISE_AUTO_INSTALL", "false")
        .env("MISE_EXEC_AUTO_INSTALL", "false")
        .args([
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.97.1",
            "--",
            &cargo,
            "package",
            "--locked",
            "--offline",
            "--no-verify",
            "--target-dir",
        ])
        .arg(target.path())
        .output()?;
    if !package.status.success() {
        return Err("fixture package failed".into());
    }
    Ok(fs::read(target.path().join("package/demo-0.1.1.crate"))?)
}

fn exercise_repair(
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &Github,
    registry: &Registry,
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let repair_tag = prepare_crate_release(intent, request, github, registry, root)?;
    assert!(matches!(
        repair_tag.packages[0].action,
        CrateReleaseAction::RepairTag { .. }
    ));
    github.approved.set(false);
    assert!(repair_crate_release(&repair_tag, intent, request, github, registry, root).is_err());
    assert_eq!(github.writes.get(), 0);
    github.approved.set(true);
    github.owner_match.set(false);
    assert!(repair_crate_release(&repair_tag, intent, request, github, registry, root).is_err());
    assert_eq!(github.writes.get(), 0);
    github.owner_match.set(true);
    registry.archive.borrow_mut().push(0);
    assert!(repair_crate_release(&repair_tag, intent, request, github, registry, root).is_err());
    assert_eq!(github.writes.get(), 0);
    registry.archive.borrow_mut().pop();
    github.release_variant.set(4);
    github.tag_exists.set(true);
    github.tag_wrong.set(true);
    assert!(repair_crate_release(&repair_tag, intent, request, github, registry, root).is_err());
    assert_eq!(github.writes.get(), 0);
    github.tag_wrong.set(false);
    github.tag_exists.set(false);
    github.write_error.set(1);
    assert!(repair_crate_release(&repair_tag, intent, request, github, registry, root).is_err());
    assert!(!github.tag_exists.get());
    assert_eq!(github.writes.get(), 1);
    github.write_error.set(3);
    assert!(repair_crate_release(&repair_tag, intent, request, github, registry, root).is_err());
    assert!(github.tag_exists.get());
    assert_eq!(github.release_variant.get(), 4);
    github.write_error.set(0);
    let after_timeout = prepare_crate_release(intent, request, github, registry, root)?;
    assert!(matches!(
        after_timeout.packages[0].action,
        CrateReleaseAction::RepairRelease { .. }
    ));
    github.tag_exists.set(false);
    repair_crate_release(&repair_tag, intent, request, github, registry, root)?;
    assert!(github.tag_exists.get());
    assert_eq!(github.release_variant.get(), 0);
    github.tag_exists.set(false);

    github.tag_exists.set(true);
    github.release_variant.set(4);
    let repair_release = prepare_crate_release(intent, request, github, registry, root)?;
    assert!(matches!(
        repair_release.packages[0].action,
        CrateReleaseAction::RepairRelease { .. }
    ));
    github.release_variant.set(0);
    let already_complete = prepare_crate_release(intent, request, github, registry, root)?;
    assert!(matches!(
        already_complete.packages[0].action,
        CrateReleaseAction::Reconciled { .. }
    ));
    Ok(())
}

#[test]
fn recovery_requires_fresh_human_approval_and_registry_absence()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    let receipt = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert!(
        verify_crate_release_admission(&receipt, &intent, &request, &github, &registry, root)
            .is_err()
    );
    github.approved.set(true);
    let mut wrong_workflow = request.clone();
    wrong_workflow.workflow_sha = "cccccccccccccccccccccccccccccccccccccccc".into();
    assert!(
        verify_crate_release_admission(
            &receipt,
            &intent,
            &wrong_workflow,
            &github,
            &registry,
            root
        )
        .is_err()
    );
    let mut wrong_source = request.clone();
    wrong_source.source_sha = "dddddddddddddddddddddddddddddddddddddddd".into();
    assert!(
        verify_crate_release_admission(&receipt, &intent, &wrong_source, &github, &registry, root)
            .is_err()
    );
    github.owner_match.set(false);
    assert!(
        verify_crate_release_admission(&receipt, &intent, &request, &github, &registry, root)
            .is_err()
    );
    github.owner_match.set(true);
    verify_crate_release_admission(&receipt, &intent, &request, &github, &registry, root)?;
    registry.absent.set(false);
    assert!(
        verify_crate_release_admission(&receipt, &intent, &request, &github, &registry, root)
            .is_err()
    );
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    exercise_repair(&intent, &request, &github, &registry, root)?;
    let completed = reconcile_crate_release(&receipt, &intent, &request, &github, &registry, root)?;
    assert_eq!(completed[0].release_id, 12);
    assert_eq!(
        completed[0].release_url,
        "https://github.com/example/demo/releases/tag/demo-v0.1.1"
    );
    assert_eq!(completed[0].tag_commit, request.source_sha);
    assert!(completed[0].source_inventory.contains_key("src/lib.rs"));
    for variant in [1, 2, 3] {
        github.release_variant.set(variant);
        assert!(
            reconcile_crate_release(&receipt, &intent, &request, &github, &registry, root).is_err()
        );
    }
    github.release_variant.set(4);
    assert!(
        velnor_actions_orchestrator::crate_release_github::read_github_release_evidence(
            "example/demo",
            "demo-v0.1.1",
            &github
        )?
        .is_none()
    );
    github.release_variant.set(0);
    registry.archive.borrow_mut().push(0);
    assert!(
        reconcile_crate_release(&receipt, &intent, &request, &github, &registry, root).is_err()
    );
    Ok(())
}

#[test]
fn source_free_github_gate_does_not_execute_cargo() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("VELNOR_SOURCE_FREE_GATE").is_some() {
        let temp = tempfile::tempdir()?;
        let (_intent, request, github, _registry) = make_fixture(temp.path())?;
        github.approved.set(true);
        let candidate = crate::crate_release_github::verify_github_evidence(&request, &github)?;
        crate::crate_release_github::authorize_github_candidate(&request, &candidate, &github)?;
        github.approved.set(false);
        assert!(
            crate::crate_release_github::authorize_github_candidate(&request, &candidate, &github)
                .is_err()
        );
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let status = Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("crate_release_controller::tests::source_free_github_gate_does_not_execute_cargo")
        .env("VELNOR_SOURCE_FREE_GATE", "1")
        .env("RUSTUP_HOME", temp.path().join("missing-rustup"))
        .env("MISE_RUSTUP_HOME", temp.path().join("missing-rustup"))
        .status()?;
    assert!(status.success());
    Ok(())
}

#[test]
fn receipt_acquire_requires_current_approval_and_matching_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_tool_artifact::{
        ToolchainStageRequest, fixture_acquire_receipt as acquire_receipt_artifact,
    };
    let source = tempfile::tempdir()?;
    let runner = tempfile::tempdir()?;
    let runner_path = runner.path().canonicalize()?;
    let (intent, request, github, registry) = make_fixture(source.path())?;
    github.approved.set(true);
    github.artifact_receipt.set(true);
    let mut receipt = velnor_actions_orchestrator::crate_release_controller::prepare_crate_release(
        &intent,
        &request,
        &github,
        &registry,
        source.path(),
    )?;
    receipt.semver_evidence = receipt
        .packages
        .iter()
        .map(|p| (p.source.name.clone(), fixture_no_baseline()))
        .collect();
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&receipt)?)?;
    let stage = ToolchainStageRequest {
        intent: &intent,
        github: &request,
        candidate: &receipt.github,
        artifact_id: 10,
        token: "test",
        runner_temp: &runner_path,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
    };
    acquire_receipt_artifact(&stage, &github, &registry, &github, source.path())?;
    assert_eq!(
        serde_json::from_slice::<
            velnor_actions_orchestrator::crate_release_controller::CrateReleaseAdmissionReceipt,
        >(&fs::read(
            runner.path().join("velnor-crate-release-receipt.json")
        )?)?,
        receipt
    );
    fs::remove_file(runner.path().join("velnor-crate-release-receipt.json"))?;
    check_receipt_revalidation(&stage, &github, &registry, &receipt, source.path())?;
    github.tag_exists.set(true);
    velnor_actions_orchestrator::crate_release_tool_artifact::acquire_reconciliation_receipt_artifact(&stage, &github, &registry, &github, source.path())?;
    fs::remove_file(runner.path().join("velnor-crate-release-receipt.json"))?;
    github.tag_wrong.set(true);
    assert!(velnor_actions_orchestrator::crate_release_tool_artifact::acquire_reconciliation_receipt_artifact(&stage, &github, &registry, &github, source.path()).is_err());
    github.tag_wrong.set(false);
    registry.inconsistent.set(true);
    assert!(
        !velnor_actions_orchestrator::crate_release_controller::validate_reconciliation_receipt(
            &receipt,
            &intent,
            &request,
            &github,
            &registry,
            source.path()
        )?
    );
    registry.inconsistent.set(false);
    registry.absent.set(true);
    assert!(velnor_actions_orchestrator::crate_release_tool_artifact::acquire_reconciliation_receipt_artifact(&stage, &github, &registry, &github, source.path()).is_err());
    registry.absent.set(false);
    github.owner_match.set(false);
    assert!(velnor_actions_orchestrator::crate_release_tool_artifact::acquire_reconciliation_receipt_artifact(&stage, &github, &registry, &github, source.path()).is_err());
    github.owner_match.set(true);
    registry.absent.set(true);
    github.tag_exists.set(false);
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec_pretty(&receipt)?)?;
    assert!(acquire_receipt_artifact(&stage, &github, &registry, &github, source.path()).is_err());
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&receipt)?)?;
    github.mutate_after_download.set(1);
    assert!(acquire_receipt_artifact(&stage, &github, &registry, &github, source.path()).is_err());
    assert!(
        !runner
            .path()
            .join("velnor-crate-release-receipt.json")
            .exists()
    );
    Ok(())
}

fn check_receipt_revalidation(
    stage: &crate::crate_release_tool_artifact::ToolchainStageRequest<'_>,
    github: &Github,
    registry: &Registry,
    receipt: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    source: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_tool_artifact::fixture_acquire_receipt;
    let mut forged = receipt.clone();
    forged.semver_evidence.clear();
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&forged)?)?;
    assert!(fixture_acquire_receipt(stage, github, registry, github, source).is_err());
    assert!(
        !stage
            .runner_temp
            .join("velnor-crate-release-receipt.json")
            .exists()
    );
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(receipt)?)?;
    *registry.archive.borrow_mut() = fixture_archive(source)?;
    registry.absent.set(false);
    fixture_acquire_receipt(stage, github, registry, github, source)?;
    verify_crate_release_admission(
        receipt,
        stage.intent,
        stage.github,
        github,
        registry,
        source,
    )?;
    fs::remove_file(stage.runner_temp.join("velnor-crate-release-receipt.json"))?;
    Ok(())
}

fn receipt_zip(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "velnor-crate-release-receipt.json",
        zip::write::SimpleFileOptions::default(),
    )?;
    zip.write_all(bytes)?;
    Ok(zip.finish()?.into_inner())
}

#[cfg(unix)]
#[test]
fn ambient_rust_wrappers_cannot_override_verified_tools() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir()?;
    let wrapper = temp.path().join("hostile-wrapper");
    fs::write(&wrapper, "#!/bin/sh\nexit 86\n")?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
    let status = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "crate_release_controller::tests::recovery_requires_fresh_human_approval_and_registry_absence",
        ])
        .env("RUSTC_WRAPPER", &wrapper)
        .env("RUSTC_WORKSPACE_WRAPPER", &wrapper)
        .env("RUSTC", &wrapper)
        .env("RUSTDOC", &wrapper)
        .env("CARGO_ENCODED_RUSTFLAGS", "--definitely-invalid")
        .status()?;
    assert!(status.success());
    Ok(())
}

#[cfg(unix)]
#[test]
fn toolchain_staging_requires_approved_same_run_artifact() -> Result<(), Box<dyn std::error::Error>>
{
    use flate2::{Compression, write::GzEncoder};
    use std::io::{Cursor, Write};
    use velnor_actions_orchestrator::crate_release_github::verify_github_evidence;
    use velnor_actions_orchestrator::crate_release_tool_artifact::{
        ToolchainStageRequest, stage_unverified_toolchain,
    };
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("source");
    fs::create_dir(&source)?;
    let (intent, request, github, _) = make_fixture(&source)?;
    github.approved.set(true);
    let candidate = verify_github_evidence(&request, &github)?;
    let host = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Ok(()),
    };
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_mode(0o755);
    header.set_cksum();
    tar.append_data(
        &mut header,
        format!("toolchains/1.97.1-{host}/bin/cargo"),
        &b"x"[..],
    )?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "rust-toolchain.tar.gz",
        zip::write::SimpleFileOptions::default(),
    )?;
    zip.write_all(&tar.into_inner()?.finish()?)?;
    *github.tool_archive.borrow_mut() = zip.finish()?.into_inner();
    let runner = temp.path().canonicalize()?.join("runner");
    fs::create_dir_all(runner.join("velnor"))?;
    let stage = ToolchainStageRequest {
        intent: &intent,
        github: &request,
        candidate: &candidate,
        artifact_id: 10,
        token: "fixture-token",
        runner_temp: &runner,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
    };
    github.mutate_after_download.set(1);
    assert!(stage_unverified_toolchain(&stage, &github, &github).is_err());
    assert!(!runner.join("velnor/rustup").exists());
    github.approved.set(true);
    github.mutate_after_download.set(2);
    assert!(stage_unverified_toolchain(&stage, &github, &github).is_err());
    assert!(!runner.join("velnor/rustup").exists());
    github.tool_archive.borrow_mut().pop();
    github.mutate_after_download.set(0);
    let result = stage_unverified_toolchain(&stage, &github, &github)?;
    assert_eq!(result, runner.join("velnor/rustup"));
    assert_eq!(
        fs::read(result.join(format!("toolchains/1.97.1-{host}/bin/cargo")))?,
        b"x"
    );
    Ok(())
}

#[test]
fn github_release_evidence_requires_a_positive_numeric_id() -> Result<(), Box<dyn std::error::Error>>
{
    let temp = tempfile::tempdir()?;
    let (_, _, github, _) = make_fixture(temp.path())?;
    github.tag_exists.set(true);
    assert!(
        velnor_actions_orchestrator::crate_release_github::read_github_release_evidence(
            "example/demo",
            "demo-v0.1.1",
            &github
        )?
        .is_some()
    );
    for variant in [5, 6, 7, 8, 9] {
        github.release_variant.set(variant);
        assert!(
            velnor_actions_orchestrator::crate_release_github::read_github_release_evidence(
                "example/demo",
                "demo-v0.1.1",
                &github
            )
            .is_err()
        );
    }
    Ok(())
}
use sha1_checked::{Digest as Sha1Digest, Sha1};
use std::collections::BTreeMap;
use velnor_actions_orchestrator::crate_release_journal::{JournalIo, JournalTag};
#[derive(Default)]
struct JournalRegistryFake {
    refs: RefCell<BTreeMap<String, String>>,
    blobs: RefCell<BTreeMap<String, Vec<u8>>>,
    tags: RefCell<BTreeMap<String, JournalTag>>,
    reject_rules: bool,
    fail_ref_after_write: bool,
    fail_ref_before_write: Option<()>,
    wrong_blob_sha: bool,
    wrong_tag_target: Option<()>,
    wrong_tag_sha: Option<()>,
    access_checks: Cell<usize>,
    write_calls: Cell<usize>,
    reject_access: Option<()>,
    after_ref_created: RefCell<Option<RefCreatedHook>>,
}

impl velnor_actions_orchestrator::crate_release_journal::JournalOperationRefs
    for JournalRegistryFake
{
    fn list_operation_refs(
        &self,
        identity: &velnor_actions_orchestrator::crate_release_journal::JournalIdentity,
    ) -> Result<Vec<String>, OrchestratorError> {
        let prefix = format!(
            "refs/tags/velnor-receipt/{}/{}/operation-",
            identity.run_id, identity.run_attempt
        );
        Ok(self
            .refs
            .borrow()
            .keys()
            .filter(|name| name.starts_with(&prefix))
            .cloned()
            .collect())
    }
}

impl JournalIo for JournalRegistryFake {
    fn check_repository_access(&self, _: &str) -> Result<(), OrchestratorError> {
        self.access_checks.set(self.access_checks.get() + 1);
        if self.reject_access.is_some() {
            Err(err())
        } else {
            Ok(())
        }
    }
    fn check_tag_rules(&self, _: &str) -> Result<(), OrchestratorError> {
        if self.reject_rules {
            Err(err())
        } else {
            Ok(())
        }
    }
    fn get_ref(&self, _: &str, reference: &str) -> Result<Option<String>, OrchestratorError> {
        Ok(self.refs.borrow().get(reference).cloned())
    }
    fn create_blob(&self, _: &str, content: &[u8]) -> Result<String, OrchestratorError> {
        self.write_calls.set(self.write_calls.get() + 1);
        let mut hasher = Sha1::new();
        hasher.update(format!("blob {}\0", content.len()).as_bytes());
        hasher.update(content);
        let id = hasher
            .try_finalize()
            .hash()
            .iter()
            .fold(String::new(), |mut output, byte| {
                output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
                output
            });
        self.blobs.borrow_mut().insert(id.clone(), content.to_vec());
        Ok(if self.wrong_blob_sha {
            "d".repeat(40)
        } else {
            id
        })
    }
    fn create_tag_object(&self, _: &str, tag: &JournalTag) -> Result<String, OrchestratorError> {
        self.write_calls.set(self.write_calls.get() + 1);
        let raw = raw_tag(tag);
        let mut hasher = Sha1::new();
        hasher.update(format!("tag {}\0", raw.len()).as_bytes());
        hasher.update(&raw);
        let id = if self.wrong_tag_sha.is_some() {
            "e".repeat(40)
        } else {
            hasher
                .try_finalize()
                .hash()
                .iter()
                .fold(String::new(), |mut output, byte| {
                    output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
                    output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
                    output
                })
        };
        self.tags.borrow_mut().insert(id.clone(), tag.clone());
        Ok(id)
    }
    fn create_ref(&self, _: &str, reference: &str, tag_sha: &str) -> Result<(), OrchestratorError> {
        self.write_calls.set(self.write_calls.get() + 1);
        if self.fail_ref_before_write.is_some() {
            return Err(err());
        }
        if self.refs.borrow().contains_key(reference) {
            return Err(err());
        }
        self.refs
            .borrow_mut()
            .insert(reference.to_owned(), tag_sha.to_owned());
        if let Some(hook) = self.after_ref_created.borrow().as_ref() {
            hook(reference);
        }
        if self.fail_ref_after_write {
            return Err(err());
        }
        Ok(())
    }
    fn get_tag_object(&self, _: &str, sha: &str) -> Result<JournalTag, OrchestratorError> {
        let mut tag = self.tags.borrow().get(sha).cloned().ok_or_else(err)?;
        if self.wrong_tag_target.is_some() {
            "commit".clone_into(&mut tag.object_type);
        }
        Ok(tag)
    }
    fn get_verified_tag_bytes(&self, _: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        Ok(raw_tag(self.tags.borrow().get(sha).ok_or_else(err)?))
    }
    fn get_blob(&self, _: &str, sha: &str) -> Result<Vec<u8>, OrchestratorError> {
        self.blobs.borrow().get(sha).cloned().ok_or_else(err)
    }
}

fn raw_tag(tag: &JournalTag) -> Vec<u8> {
    format!(
        "object {}\ntype {}\ntag {}\ntagger Velnor Release <release@velnor.invalid> 0 +0000\n\n{}",
        tag.object_sha, tag.object_type, tag.name, tag.message
    )
    .into_bytes()
}

fn err() -> OrchestratorError {
    OrchestratorError::Contract {
        problem: "fake_remote_failure".into(),
    }
}

#[test]
fn observed_upload_is_derived_from_current_registry() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        CrateReleaseObservation, record_verified_mutation,
    };
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, inspect_journal_mutation,
        record_journal_intent, record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    let obs = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 2,
    };
    let mut wrong_identity = identity.clone();
    wrong_identity.workflow_sha = "c".repeat(40);
    let wrong = CrateReleaseObservation {
        identity: &wrong_identity,
        ..obs
    };
    assert!(record_verified_mutation(&wrong).is_err());
    reject_wrong_authentication(&obs);
    let mut wrong_intent = intent.clone();
    wrong_intent.rust_version = "1.99.1".into();
    let wrong = CrateReleaseObservation {
        intent: &wrong_intent,
        ..obs
    };
    assert!(record_verified_mutation(&wrong).is_err());
    let mut wrong_request = request.clone();
    wrong_request.source_sha = "d".repeat(40);
    let wrong = CrateReleaseObservation {
        request: &wrong_request,
        ..obs
    };
    assert!(record_verified_mutation(&wrong).is_err());
    github.owner_match.set(false);
    assert!(record_verified_mutation(&obs).is_err());
    github.owner_match.set(true);
    reject_tampered_source(&obs)?;
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    github.approved.set(false);
    assert!(record_verified_mutation(&obs).is_err());
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    github.approved.set(true);
    registry.archive.borrow_mut().push(0);
    assert!(record_verified_mutation(&obs).is_err());
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    registry.archive.borrow_mut().pop();
    record_verified_mutation(&obs)?;
    verify_upload_result(&obs)?;
    verify_metadata_observations(&obs, &github)?;
    Ok(())
}

fn reject_tampered_source(
    obs: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = obs.source.join("src/lib.rs");
    fs::write(&path, "tampered")?;
    assert!(
        velnor_actions_orchestrator::crate_release_controller::record_verified_mutation(obs)
            .is_err()
    );
    fs::write(path, "pub fn demo() {}")?;
    Ok(())
}

fn reject_wrong_authentication(
    obs: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) {
    let mut wrong_auth = obs.journal_receipt.clone();
    assert!(super::observation::verify_binding(obs).is_ok());
    wrong_auth.authentication =
        velnor_actions_orchestrator::crate_release_journal::JournalAuthentication::BootstrapToken;
    let wrong = velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation {
        journal_receipt: &wrong_auth,
        ..*obs
    };
    assert!(super::observation::verify_binding(&wrong).is_err());
}

fn fixture_journal_plan(
    root: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    admission: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<
    (
        velnor_actions_orchestrator::crate_release_journal::JournalIdentity,
        velnor_actions_orchestrator::crate_release_journal::JournalReceipt,
    ),
    Box<dyn std::error::Error>,
> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalAuthentication, JournalIdentity, JournalOwner, JournalOwnerKind, JournalPackage,
        JournalReceipt, JournalRegistry, JournalToolPins,
    };
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let intent_sha256 = test_hex(&Sha256::digest(
        velnor_actions_contract::canonical_json_bytes(intent)?,
    ));
    let mut base_intent = intent.clone();
    base_intent.journal_predecessor = None;
    base_intent.recovery = None;
    let base_intent_sha256 = test_hex(&Sha256::digest(
        velnor_actions_contract::canonical_json_bytes(&base_intent)?,
    ));
    let plan = JournalReceipt {
        semver_evidence: admission.semver_evidence.clone(),
        predecessor: None,
        identity: identity.clone(),
        base_intent_sha256,
        intent_sha256,
        registry: JournalRegistry::CratesIo,
        generator_version: "0.1.72".into(),
        release_plz_version: "0.4.3".into(),
        rust_version: "1.97.1".into(),
        expected_tools: JournalToolPins {
            generator_sha256: "1".repeat(64),
            release_plz_sha256: "2".repeat(64),
            rust_manifest_sha256: "3".repeat(64),
        },
        authentication: JournalAuthentication::TrustedPublishing,
        required_check_id: admission.github.required_check_id,
        trusted_reviewer_id: 139_017,
        workflow_run_url: "https://github.com/example/demo/actions/runs/12".into(),
        source_files: velnor_actions_orchestrator::journal_source_inventory(
            root,
            &request.source_sha,
        )?,
        packages: vec![JournalPackage {
            name: "demo".into(),
            version: "0.1.1".into(),
            manifest: "Cargo.toml".into(),
            tag: "demo-v0.1.1".into(),
            owners: crate::crate_release_journal::JournalOwnership::Existing {
                owners: vec![JournalOwner {
                    id: 370_532,
                    login: "owner".into(),
                    kind: JournalOwnerKind::User,
                }],
            },
        }],
    };
    Ok((identity, plan))
}

fn verify_metadata_observations(
    prior: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    github: &Github,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        CrateReleaseObservation, record_verified_mutation,
    };
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind as Kind, JournalMutationPhase as Phase,
        JournalMutationResult as ResultKind, inspect_journal_mutation, record_journal_intent,
    };
    let mut intent = JournalIntendedMutation {
        semver_evidence: None,
        sequence: 3,
        package: "demo".into(),
        version: "0.1.1".into(),
        kind: Kind::CreateTag,
    };
    record_journal_intent(
        prior.journal,
        prior.identity,
        prior.journal_receipt,
        &intent,
    )?;
    let tag_observation = CrateReleaseObservation {
        sequence: 4,
        ..*prior
    };
    github.tag_exists.set(true);
    github.tag_wrong.set(true);
    assert!(record_verified_mutation(&tag_observation).is_err());
    assert!(inspect_journal_mutation(prior.journal, prior.identity, 4)?.is_none());
    github.tag_wrong.set(false);
    record_verified_mutation(&tag_observation)?;
    let tag = inspect_journal_mutation(prior.journal, prior.identity, 4)?
        .ok_or("missing tag evidence")?;
    assert!(
        matches!(tag.phase, Phase::Observed { result: ResultKind::Tag { ref name, ref target_sha } } if name == "demo-v0.1.1" && target_sha == &prior.request.source_sha)
    );
    intent.sequence = 5;
    intent.kind = Kind::CreateRelease;
    record_journal_intent(
        prior.journal,
        prior.identity,
        prior.journal_receipt,
        &intent,
    )?;
    let release_observation = CrateReleaseObservation {
        sequence: 6,
        ..*prior
    };
    github.release_variant.set(4);
    assert!(record_verified_mutation(&release_observation).is_err());
    assert!(inspect_journal_mutation(prior.journal, prior.identity, 6)?.is_none());
    github.release_variant.set(0);
    record_verified_mutation(&release_observation)?;
    let release = inspect_journal_mutation(prior.journal, prior.identity, 6)?
        .ok_or("missing release evidence")?;
    assert!(
        matches!(release.phase, Phase::Observed { result: ResultKind::Release { id: 12, ref tag, ref url } } if tag == "demo-v0.1.1" && url == "https://github.com/example/demo/releases/tag/demo-v0.1.1")
    );
    verify_reconciled_candidate(prior, github)?;
    Ok(())
}

fn test_hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        result.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    result
}

fn verify_upload_result(
    obs: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalMutationPhase, JournalMutationResult, inspect_journal_mutation,
    };
    let observed =
        inspect_journal_mutation(obs.journal, obs.identity, 2)?.ok_or("missing observation")?;
    let JournalMutationPhase::Observed {
        result:
            JournalMutationResult::Upload {
                registry_checksum,
                downloaded_checksum,
                owners,
                features,
                files,
            },
    } = observed.phase
    else {
        return Err("wrong observed result".into());
    };
    let archive = obs.registry.archive.borrow();
    let expected = test_hex(&Sha256::digest(archive.as_slice()));
    assert_eq!(registry_checksum, expected);
    assert_eq!(downloaded_checksum, expected);
    assert_eq!(
        owners,
        journal_existing_owners(&obs.journal_receipt.packages[0].owners)
    );
    assert_eq!(features.get("demo-feature"), Some(&Vec::new()));
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive.as_slice()));
    let mut expected_files = Vec::new();
    for entry in tar.entries()? {
        use std::io::Read as _;
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()?
            .to_string_lossy()
            .strip_prefix("demo-0.1.1/")
            .ok_or("bad prefix")?
            .to_owned();
        let executable = entry.header().mode()? & 0o111 != 0;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        expected_files.push(
            velnor_actions_orchestrator::crate_release_journal::JournalFile {
                path,
                sha256: test_hex(&Sha256::digest(&bytes)),
                kind:
                    velnor_actions_orchestrator::crate_release_journal::JournalFileKind::Regular {
                        executable,
                    },
            },
        );
    }
    expected_files.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(files, expected_files);
    drop(archive);
    Ok(())
}

#[test]
fn journal_observation_runtime_is_blocked_until_authority_is_qualified() {
    for operation in [
        "journal-observe",
        "journal-coordinate",
        "journal-start",
        "journal-repair-tag",
        "journal-repair-release",
        "journal-upload-intent",
        "journal-recovery-inspect",
        "journal-reconcile-evidence",
    ] {
        let error = crate::crate_release_runtime::run_crate_release(operation)
            .expect_err("guard must reject");
        assert!(
            error
                .to_string()
                .contains("crate_release_authority_not_qualified")
        );
    }
}

#[test]
fn prior_upload_recovery_reobserves_registry_without_writes()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation;
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let mut admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, mut current) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let mut previous = current.clone();
    previous.identity.run_id = 11;
    previous.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    current.predecessor = Some(previous.identity.clone());
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: previous.identity.run_id,
        run_attempt: previous.identity.run_attempt,
        workflow_sha: previous.identity.workflow_sha.clone(),
    });
    admission.intent = intent.clone();
    current.intent_sha256 = test_hex(&Sha256::digest(
        velnor_actions_contract::canonical_json_bytes(&intent)?,
    ));
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &previous.identity, &previous, root)?;
    record_journal_intent(
        &journal,
        &previous.identity,
        &previous,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    record_source_bound_start(&journal, &identity, &current, root)?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &current,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 2,
    };
    assert_prior_observation_states(&input, &previous)?;
    Ok(())
}

fn assert_prior_observation_states(
    input: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    previous: &velnor_actions_orchestrator::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        RecoveryState, observe_prior_recovery,
    };
    let before = input.journal.refs.borrow().len();
    let missing = observe_prior_recovery(input, &previous.identity)?;
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].state, RecoveryState::Missing);
    assert_eq!(
        serde_json::to_value(&missing[0])?["prior_record"],
        "intention-only"
    );
    assert_eq!(input.journal.refs.borrow().len(), before);
    *input.registry.archive.borrow_mut() = fixture_archive(input.source)?;
    input.registry.absent.set(false);
    let present = observe_prior_recovery(input, &previous.identity)?;
    assert_eq!(present[0].state, RecoveryState::Present);
    assert_eq!(input.journal.refs.borrow().len(), before);
    append_fixture_prior_upload_observation(input.journal, previous)?;
    let before = input.journal.refs.borrow().len();
    let recorded = observe_prior_recovery(input, &previous.identity)?;
    assert_eq!(
        serde_json::to_value(&recorded[0])?["prior_record"],
        "observation-recorded"
    );
    assert_eq!(recorded[0].state, RecoveryState::Present);
    assert_eq!(input.journal.refs.borrow().len(), before);
    input.github.approved.set(false);
    assert!(observe_prior_recovery(input, &previous.identity).is_err());
    input.github.approved.set(true);
    input.registry.archive.borrow_mut().push(0);
    assert!(observe_prior_recovery(input, &previous.identity).is_err());
    assert_eq!(input.journal.refs.borrow().len(), before);
    Ok(())
}

fn append_fixture_prior_upload_observation(
    journal: &JournalRegistryFake,
    receipt: &velnor_actions_orchestrator::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalFile, JournalFileKind, JournalMutation, JournalMutationKind, JournalMutationPhase,
        JournalMutationResult,
    };
    let mutation = JournalMutation {
        semver_evidence: None,
        sequence: 2,
        package: "demo".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        phase: JournalMutationPhase::Observed {
            result: JournalMutationResult::Upload {
                registry_checksum: "d".repeat(64),
                downloaded_checksum: "d".repeat(64),
                owners: journal_existing_owners(&receipt.packages[0].owners),
                features: BTreeMap::new(),
                files: vec![JournalFile {
                    path: "src/lib.rs".into(),
                    sha256: "e".repeat(64),
                    kind: JournalFileKind::Regular { executable: false },
                }],
            },
        },
    };
    let body = velnor_actions_contract::canonical_json_bytes(
        &json!({"schema":1,"identity":receipt.identity,"receipt":receipt,"mutation":mutation}),
    )?;
    let repository = &receipt.identity.repository;
    let blob = journal.create_blob(repository, &body)?;
    let name = format!(
        "velnor-receipt/{}/{}/operation-2",
        receipt.identity.run_id, receipt.identity.run_attempt
    );
    let tag = JournalTag {
        name: name.clone(),
        object_type: "blob".into(),
        object_sha: blob,
        message: "velnor-release-receipt-v1".into(),
    };
    let tag_sha = journal.create_tag_object(repository, &tag)?;
    journal.create_ref(repository, &format!("refs/tags/{name}"), &tag_sha)?;
    Ok(())
}

fn verify_reconciled_candidate(
    input: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    github: &Github,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::derive_reconciled_candidate;
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
    };
    let before = input.journal.refs.borrow().len();
    let mut wrong_identity = input.identity.clone();
    wrong_identity.run_id += 1;
    let wrong = velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation {
        identity: &wrong_identity,
        ..*input
    };
    assert!(derive_reconciled_candidate(&wrong).is_err());
    let mut wrong_intent = input.intent.clone();
    wrong_intent.rust_version = "1.99.1".into();
    let wrong = velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation {
        intent: &wrong_intent,
        ..*input
    };
    assert!(derive_reconciled_candidate(&wrong).is_err());
    github.owner_match.set(false);
    assert!(derive_reconciled_candidate(input).is_err());
    github.owner_match.set(true);
    fs::write(input.source.join("src/lib.rs"), "tampered")?;
    assert!(derive_reconciled_candidate(input).is_err());
    fs::write(input.source.join("src/lib.rs"), "pub fn demo() {}")?;
    let candidate = derive_reconciled_candidate(input)?;
    assert_eq!(candidate.history.len(), 6);
    assert_eq!(candidate.packages.len(), 1);
    assert_eq!(candidate.packages[0].release_id, 12);
    assert_eq!(
        candidate.packages[0].registry.features.get("demo-feature"),
        Some(&Vec::new())
    );
    assert!(
        candidate.packages[0]
            .source_inventory
            .contains_key("src/lib.rs")
    );
    assert_eq!(input.journal.refs.borrow().len(), before);
    github.approved.set(false);
    assert!(derive_reconciled_candidate(input).is_err());
    github.approved.set(true);
    github.tag_wrong.set(true);
    assert!(derive_reconciled_candidate(input).is_err());
    github.tag_wrong.set(false);
    github.release_variant.set(5);
    assert!(derive_reconciled_candidate(input).is_err());
    github.release_variant.set(0);
    input.registry.archive.borrow_mut().push(0);
    assert!(derive_reconciled_candidate(input).is_err());
    input.registry.archive.borrow_mut().pop();
    assert_eq!(input.journal.refs.borrow().len(), before);
    record_journal_intent(
        input.journal,
        input.identity,
        input.journal_receipt,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 7,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    assert!(derive_reconciled_candidate(input).is_err());
    Ok(())
}

#[test]
fn reconciled_candidate_allows_verified_state_without_mutation_history()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        CrateReleaseObservation, derive_reconciled_candidate,
    };
    use velnor_actions_orchestrator::crate_release_journal::record_source_bound_start;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.tag_exists.set(true);
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 0,
    };
    let before = journal.refs.borrow().len();
    let candidate = derive_reconciled_candidate(&input)?;
    assert_eq!(candidate.history.len(), 0);
    assert_eq!(candidate.packages.len(), 1);
    assert_eq!(candidate.packages[0].release_id, 12);
    assert_eq!(journal.refs.borrow().len(), before);
    Ok(())
}

#[test]
fn upload_intent_requires_fresh_admission_and_absent_publication()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation;
    use velnor_actions_orchestrator::crate_release_journal::record_source_bound_start;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    let obs = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    check_upload_semver_rejections(&obs)?;
    check_upload_authorization(&obs)
}

fn check_upload_authorization(
    obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use super::observation::record_admitted_upload_intent;
    use crate::crate_release_controller::{
        verify_and_record_upload_intent, verify_release_package,
    };
    use crate::crate_release_journal::{JournalMutationPhase, inspect_journal_mutation};
    let (admission, intent, request, github, registry, root, journal, identity) = (
        obs.admission,
        obs.intent,
        obs.request,
        obs.github,
        obs.registry,
        obs.source,
        obs.journal,
        obs.identity,
    );
    let prove =
        || verify_release_package(admission, intent, request, github, registry, root, "demo");
    assert!(
        verify_release_package(admission, intent, request, github, registry, root, "other")
            .is_err()
    );
    let verified = prove()?;
    github.approved.set(false);
    assert!(
        record_admitted_upload_intent(
            obs,
            verified,
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    github.approved.set(true);
    let verified = prove()?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    assert!(
        record_admitted_upload_intent(
            obs,
            verified,
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    registry.absent.set(true);
    assert!(inspect_journal_mutation(journal, identity, 1)?.is_none());
    check_upload_identity_rejections(obs)?;
    assert!(inspect_journal_mutation(journal, identity, 1)?.is_none());
    assert!(
        verify_and_record_upload_intent(
            obs,
            "other",
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    assert!(inspect_journal_mutation(journal, identity, 1)?.is_none());
    verify_and_record_upload_intent(
        obs,
        "demo",
        crate::crate_release_semver::fixture_verified_semver(admission, "demo")?,
    )?;
    assert!(matches!(
        inspect_journal_mutation(journal, identity, 1)?
            .ok_or("missing intention")?
            .phase,
        JournalMutationPhase::Intended
    ));
    Ok(())
}

fn check_upload_identity_rejections(
    obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use super::observation::record_admitted_upload_intent;
    use crate::crate_release_controller::{CrateReleaseObservation, verify_release_package};
    let (admission, intent, request, github, registry, root) = (
        obs.admission,
        obs.intent,
        obs.request,
        obs.github,
        obs.registry,
        obs.source,
    );
    let prove =
        || verify_release_package(admission, intent, request, github, registry, root, "demo");
    let verified = prove()?;
    let mut wrong_request = request.clone();
    wrong_request.source_sha = "d".repeat(40);
    let wrong = CrateReleaseObservation {
        request: &wrong_request,
        ..*obs
    };
    assert!(
        record_admitted_upload_intent(
            &wrong,
            verified,
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    let mutations: [fn(&mut super::VerifiedReleasePackage); 5] = [
        |proof| proof.package = "other".into(),
        |proof| proof.version = "0.1.2".into(),
        |proof| proof.intent_sha256 = "e".repeat(64),
        |proof| proof.source_sha = "f".repeat(40),
        |proof| proof.run_id += 1,
    ];
    for mutate in mutations {
        let mut verified = prove()?;
        mutate(&mut verified);
        assert!(
            record_admitted_upload_intent(
                obs,
                verified,
                crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
            )
            .is_err()
        );
    }
    Ok(())
}

fn check_upload_semver_rejections(
    obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{
        observation::record_admitted_upload_intent, verify_release_package,
    };
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
    };
    for evidence in [
        None,
        Some(crate::crate_release_semver::SemverEvidence::Deferred {
            predecessors: vec!["other".into()],
        }),
    ] {
        let intended = JournalIntendedMutation {
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            semver_evidence: evidence,
        };
        let before = obs.journal.write_calls.get();
        assert!(
            record_journal_intent(obs.journal, obs.identity, obs.journal_receipt, &intended)
                .is_err()
        );
        assert_eq!(obs.journal.write_calls.get(), before);
    }
    let mut forged = obs.admission.clone();
    forged.github.request.source_sha = "f".repeat(40);
    let semver = crate::crate_release_semver::fixture_verified_semver(&forged, "demo")?;
    let verified = verify_release_package(
        obs.admission,
        obs.intent,
        obs.request,
        obs.github,
        obs.registry,
        obs.source,
        "demo",
    )?;
    let before = obs.journal.write_calls.get();
    assert!(record_admitted_upload_intent(obs, verified, semver).is_err());
    assert_eq!(obs.journal.write_calls.get(), before);
    Ok(())
}

struct WorkspaceGithub(Github, Cell<bool>);
impl GitHubAdmissionRead for WorkspaceGithub {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let sha = &self.0.sha;
        if route.ends_with("/actions/runs/12") {
            return Ok(
                json!({"id":12,"run_attempt":1,"head_sha":sha,"head_branch":"main","event":"push","path":".github/workflows/release.yml","repository":{"id":22,"full_name":"example/demo"},"head_repository":{"id":22,"full_name":"example/demo"}}),
            );
        }
        if route.contains("/pulls?per_page=100&page=1") {
            return Ok(
                json!([{"number":17,"merged_at":"2026-11-03T16:00:00Z","merge_commit_sha":sha,"base":{"ref":"main","repo":{"full_name":"example/demo"}},"head":{"ref":"release-plz-1","repo":{"full_name":"example/demo"},"sha":sha},"user":{"login":"release-bot"}}]),
            );
        }
        if route.contains("/git/commits/") {
            return Ok(json!({"sha":sha,"tree":{"sha":"a".repeat(40)}}));
        }
        if route.contains("/pulls/17/reviews?") {
            return Ok(
                json!([{"id":1,"user":{"id":139_017,"login":"owner","type":"User"},"state":"APPROVED","commit_id":sha,"author_association":"OWNER","submitted_at":"2026-11-03T15:50:00Z"}]),
            );
        }
        if route.ends_with("/collaborators/owner/permission") {
            return Ok(json!({"user":{"id":139_017,"type":"User"},"permission":"admin"}));
        }
        self.0.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        if self.1.get() && route.contains("velnor_fixture_order_20261104_a-v0.1.1") {
            let tag = "velnor_fixture_order_20261104_a-v0.1.1";
            if route.contains("/releases/tags/") {
                return Ok(Some(
                    json!({"id":12,"tag_name":tag,"draft":false,"html_url":format!("https://github.com/example/demo/releases/tag/{tag}")}),
                ));
            }
            return Ok(Some(
                json!({"ref":format!("refs/tags/{tag}"),"object":{"type":"commit","sha":self.0.sha}}),
            ));
        }
        Ok(None)
    }
}
struct WorkspaceRegistry {
    visible: Cell<bool>,
    index_visible: Cell<bool>,
    archive: RefCell<Vec<u8>>,
}
impl CrateRegistryRead for WorkspaceRegistry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        let name = "velnor_fixture_order_20261104_a";
        if path == format!("/index/ve/ln/{name}") {
            return Ok(if self.index_visible.get() {
                CrateRegistryResponse { status: 200, body: json!({"name":name,"vers":"0.1.1","cksum":test_hex(&Sha256::digest(&*self.archive.borrow())),"yanked":false}).to_string().into_bytes() }
            } else {
                CrateRegistryResponse {
                    status: 404,
                    body: Vec::new(),
                }
            });
        }
        if self.visible.get() && path == format!("/crates/{name}/{name}-0.1.1.crate") {
            return Ok(CrateRegistryResponse {
                status: 200,
                body: self.archive.borrow().clone(),
            });
        }
        if self.visible.get() && path == format!("/api/v1/crates/{name}/0.1.1") {
            return Ok(CrateRegistryResponse {status:200,body:json!({"version":{"id":2,"crate":name,"num":"0.1.1","yanked":false,"checksum":test_hex(&Sha256::digest(&*self.archive.borrow())),"features":{"pty":[],"base":[],"default":["base"]}}}).to_string().into_bytes()});
        }
        let body = if path.ends_with("/owners") {
            json!({"users":[{"id":370_532,"login":"owner","kind":"user"}]})
        } else if path.ends_with("/0.1.1") {
            return Ok(CrateRegistryResponse {
                status: 404,
                body: Vec::new(),
            });
        } else {
            let name = path.rsplit('/').next().ok_or_else(err)?;
            if self.visible.get() && name == "velnor_fixture_order_20261104_a" {
                json!({"crate":{"id":name,"name":name,"versions":[1,2]},"versions":[{"id":1,"crate":name,"num":"0.1.0","yanked":false},{"id":2,"crate":name,"num":"0.1.1","yanked":false}]})
            } else {
                json!({"crate":{"id":name,"name":name,"versions":[1]},"versions":[{"id":1,"crate":name,"num":"0.1.0","yanked":false}]})
            }
        };
        Ok(CrateRegistryResponse {
            status: 200,
            body: body.to_string().into_bytes(),
        })
    }
}

type WorkspaceOrderFixture = (
    tempfile::TempDir,
    CrateReleaseIntent,
    GitHubAdmissionRequest,
    WorkspaceGithub,
    WorkspaceRegistry,
);

fn make_workspace_order_fixture() -> Result<WorkspaceOrderFixture, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, mut request, github, _) = make_fixture(root)?;
    fs::remove_file(root.join("src/lib.rs"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers=['a','b']\nresolver='2'\n",
    )?;
    for name in ["a", "b"] {
        fs::create_dir_all(root.join(name).join("src"))?;
        fs::write(
            root.join(name).join("src/lib.rs"),
            if name == "a" {
                "#[cfg(not(feature = \"pty\"))] compile_error!(\"pty required\");\n#[cfg(not(feature = \"base\"))] compile_error!(\"base required\");\npub fn f() {}\n"
            } else {
                "pub fn f() {}\n"
            },
        )?;
        let dep = if name == "b" {
            "[dependencies]\nvelnor_fixture_order_20261104_a={path='../a',version='0.1.1',features=['pty']}\n"
        } else {
            ""
        };
        fs::write(
            root.join(name).join("Cargo.toml"),
            format!(
                "[package]\nname='velnor_fixture_order_20261104_{name}'\nversion='0.1.1'\nedition='2021'\ndescription='fixture'\nlicense='MIT'\n[features]\npty=[]\nbase=[]\ndefault=['base']\n{dep}"
            ),
        )?;
    }
    let lock = Command::new(env!("CARGO"))
        .args(["generate-lockfile", "--offline"])
        .current_dir(root)
        .output()?;
    assert!(
        lock.status.success(),
        "{}",
        String::from_utf8_lossy(&lock.stderr)
    );
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-q", "-s", "-m", "workspace"])?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    intent.packages = vec![
        "velnor_fixture_order_20261104_a".into(),
        "velnor_fixture_order_20261104_b".into(),
    ];
    intent.publish_features = intent
        .packages
        .iter()
        .map(|name| (name.clone(), Vec::new()))
        .collect();
    intent
        .publish_features
        .insert("velnor_fixture_order_20261104_a".into(), vec!["pty".into()]);
    intent.recovery = None;
    request.event = "push".into();
    request.workflow_sha = sha.clone();
    request.source_sha = sha.clone();
    request.recovery_sha = None;
    let mut base = github;
    base.sha = sha.clone();
    base.policy_sha = sha;
    base.approved.set(true);
    let github = WorkspaceGithub(base, Cell::new(false));
    let registry = WorkspaceRegistry {
        visible: Cell::new(false),
        index_visible: Cell::new(false),
        archive: RefCell::new(Vec::new()),
    };
    Ok((temp, intent, request, github, registry))
}

#[test]
fn multi_crate_absent_predecessor_does_not_block_observation()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, intent, request, github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert_eq!(
        admission
            .packages
            .iter()
            .map(|p| p.source.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "velnor_fixture_order_20261104_a",
            "velnor_fixture_order_20261104_b"
        ]
    );
    assert!(
        admission
            .packages
            .iter()
            .all(|p| p.action == CrateReleaseAction::Publish)
    );
    let error = super::verify_release_package(
        &admission,
        &intent,
        &request,
        &github,
        &registry,
        root,
        "velnor_fixture_order_20261104_b",
    )
    .expect_err("unpublished predecessor must block verification");
    assert!(
        error
            .to_string()
            .contains("crate_release_package_not_ready")
    );
    super::verify_release_package(
        &admission,
        &intent,
        &request,
        &github,
        &registry,
        root,
        "velnor_fixture_order_20261104_a",
    )?;
    verify_after_index_visibility(root, &intent, &request, &github, &registry, &admission)
}

fn verify_after_index_visibility(
    root: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &WorkspaceGithub,
    registry: &WorkspaceRegistry,
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    let archive_dir = tempfile::tempdir()?;
    let source = admission
        .packages
        .iter()
        .find(|p| p.source.name.ends_with("_a"))
        .ok_or("a")?;
    *registry.archive.borrow_mut() = super::package_source(
        root,
        intent,
        &source.source,
        archive_dir.path(),
        &request.source_sha,
        false,
    )?;
    let server = SparseFixture::start(
        "velnor_fixture_order_20261104_a",
        registry.archive.borrow().clone(),
    )?;
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    registry.visible.set(true);
    github.1.set(true);
    let error = super::verify_release_package(
        admission,
        intent,
        request,
        github,
        registry,
        root,
        "velnor_fixture_order_20261104_b",
    )
    .expect_err("missing index must block verification");
    assert!(error.to_string().contains("crate_release_package_failed"));
    server
        .index_visible
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let result = super::verify_release_package(
        admission,
        intent,
        request,
        github,
        registry,
        root,
        "velnor_fixture_order_20261104_b",
    );
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    result?;
    assert!(server.index_hits.load(std::sync::atomic::Ordering::SeqCst) >= 2);
    assert!(
        server
            .download_hits
            .load(std::sync::atomic::Ordering::SeqCst)
            >= 1
    );
    Ok(())
}

thread_local! { static FIXTURE_SPARSE_INDEX: RefCell<Option<String>> = const { RefCell::new(None) }; }

pub(super) fn configure_test_registry(home: &Path) -> Result<(), OrchestratorError> {
    FIXTURE_SPARSE_INDEX.with(|value| {
        if let Some(url) = value.borrow().as_ref() {
            fs::write(home.join("config.toml"), format!("[source.crates-io]\nreplace-with='fixture'\n[source.fixture]\nregistry='sparse+{url}/'\n")).map_err(|_| OrchestratorError::Contract { problem: "fixture registry config".into() })?;
        }
        Ok(())
    })
}

struct SparseFixture {
    url: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    index_visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
    index_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    download_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl SparseFixture {
    fn start(name: &str, archive: Vec<u8>) -> Result<Self, Box<dyn std::error::Error>> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let url_server = url.clone();
        let name = name.to_owned();
        let checksum = test_hex(&Sha256::digest(&archive));
        let stop = Arc::new(AtomicBool::new(false));
        let stop_server = stop.clone();
        let index_visible = Arc::new(AtomicBool::new(false));
        let index_server = index_visible.clone();
        let index_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let index_hits_server = index_hits.clone();
        let download_hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let download_hits_server = download_hits.clone();
        let thread = std::thread::spawn(move || {
            let mut workers = Vec::new();
            while !stop_server.load(Ordering::Relaxed) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    continue;
                };
                let request = SparseRequest {
                    name: name.clone(),
                    url: url_server.clone(),
                    checksum: checksum.clone(),
                    archive: archive.clone(),
                    visible: index_server.clone(),
                    index_hits: index_hits_server.clone(),
                    download_hits: download_hits_server.clone(),
                    stop: stop_server.clone(),
                };
                workers.push(std::thread::spawn(move || {
                    serve_sparse_request(stream, request);
                }));
            }
            for worker in workers {
                worker.join().expect("fixture connection");
            }
        });
        Ok(Self {
            url,
            stop,
            thread: Some(thread),
            index_visible,
            index_hits,
            download_hits,
        })
    }
}
struct SparseRequest {
    name: String,
    url: String,
    checksum: String,
    archive: Vec<u8>,
    visible: std::sync::Arc<std::sync::atomic::AtomicBool>,
    index_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    download_hits: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

fn serve_sparse_request(mut stream: std::net::TcpStream, fixture: SparseRequest) {
    use std::io::{Read, Write};
    use std::sync::atomic::Ordering;
    stream
        .set_read_timeout(Some(std::time::Duration::from_millis(100)))
        .expect("set read timeout");
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") && bytes.len() < 16 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(&chunk[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) && !fixture.stop.load(Ordering::Relaxed) => {}
            Err(_) => break,
        }
    }
    let request = String::from_utf8_lossy(&bytes);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("");
    if path.is_empty() {
        return;
    }
    let name = &fixture.name;
    let (status, body) = if path == "/config.json" {
        (
            "200 OK",
            json!({"dl":format!("{}/download/{{crate}}/{{version}}", fixture.url),"api":null})
                .to_string()
                .into_bytes(),
        )
    } else if path == format!("/{}/{}/{}", &name[..2], &name[2..4], name)
        || path == format!("/{}/{}/{}", &name[..2], &name[2..4], name.replace('_', "-"))
    {
        fixture.index_hits.fetch_add(1, Ordering::SeqCst);
        if fixture.visible.load(Ordering::SeqCst) {
            ("200 OK", format!("{}\n",json!({"name":name,"vers":"0.1.1","deps":[],"cksum":fixture.checksum,"features":{"pty":[],"base":[],"default":["base"]},"yanked":false})).into_bytes())
        } else {
            ("404 Not Found", Vec::new())
        }
    } else if path == format!("/download/{name}/0.1.1") {
        fixture.download_hits.fetch_add(1, Ordering::SeqCst);
        ("200 OK", fixture.archive)
    } else {
        ("404 Not Found", Vec::new())
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .expect("write response header");
    stream.write_all(&body).expect("write response body");
}

impl Drop for SparseFixture {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("join fixture server");
        }
    }
}

#[test]
fn admitted_start_derives_receipt_and_rechecks_authority() -> Result<(), Box<dyn std::error::Error>>
{
    use crate::crate_release_journal::{JournalAuthentication, JournalStage, inspect_journal};
    use crate::package_release_signer::{
        AdmittedStart, fixture_attested_generator, record_admitted_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let journal = JournalRegistryFake::default();
    let generator = fixture_attested_generator();
    let input = AdmittedStart {
        journal: &journal,
        github: &github,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: root,
        workflow_root: root,
        generator: &generator,
    };
    github.approved.set(false);
    assert!(record_admitted_start(&input).is_err());
    assert!(journal.refs.borrow().is_empty());
    github.approved.set(true);
    github.owner_match.set(false);
    assert!(record_admitted_start(&input).is_err());
    assert!(journal.refs.borrow().is_empty());
    github.owner_match.set(true);
    let mut wrong = intent.clone();
    wrong.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    let bad = AdmittedStart {
        intent: &wrong,
        ..input
    };
    assert!(record_admitted_start(&bad).is_err());
    assert!(journal.refs.borrow().is_empty());
    record_admitted_start(&input)?;
    let (identity, _) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let recorded =
        inspect_journal(&journal, &identity, JournalStage::Started)?.ok_or("missing start")?;
    assert_eq!(
        recorded.receipt.authentication,
        JournalAuthentication::TrustedPublishing
    );
    assert_eq!(
        recorded.receipt.expected_tools.generator_sha256,
        "1".repeat(64)
    );
    assert_eq!(recorded.receipt.packages.len(), 1);
    record_admitted_start(&input)?;
    assert_eq!(journal.refs.borrow().len(), 1);
    Ok(())
}

fn reject_invalid_metadata_prerequisites(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) {
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::JournalMutationKind;
    let github = input.github;
    let registry = input.registry;
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateRelease).is_err());
    github.approved.set(false);
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateTag).is_err());
    github.approved.set(true);
    github.owner_match.set(false);
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateTag).is_err());
    github.owner_match.set(true);
    registry.archive.borrow_mut().push(0);
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateTag).is_err());
    registry.archive.borrow_mut().pop();
    github.tag_exists.set(true);
    github.tag_wrong.set(true);
    assert!(repair_verified_metadata(input, "demo", JournalMutationKind::CreateTag).is_err());
    github.tag_wrong.set(false);
    github.tag_exists.set(false);
    let invalid_sequence = CrateReleaseObservation {
        sequence: 1000,
        ..*input
    };
    assert!(
        repair_verified_metadata(&invalid_sequence, "demo", JournalMutationKind::CreateTag)
            .is_err()
    );
}

#[test]
fn metadata_repair_journals_each_create_before_and_after_write()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::{
        JournalMutationKind, JournalMutationPhase, inspect_journal_mutation,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = std::rc::Rc::new(JournalRegistryFake::default());
    record_source_bound_start(journal.as_ref(), &identity, &plan, root)?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.release_variant.set(4);
    let observed_journal = journal.clone();
    let observed_identity = identity.clone();
    *github.before_metadata_write.borrow_mut() = Some(Box::new(move |sequence| {
        let intended =
            inspect_journal_mutation(observed_journal.as_ref(), &observed_identity, sequence)
                .expect("read intention")
                .expect("intention exists");
        assert!(matches!(intended.phase, JournalMutationPhase::Intended));
        assert!(
            inspect_journal_mutation(observed_journal.as_ref(), &observed_identity, sequence + 1)
                .expect("read observation")
                .is_none()
        );
    }));
    let input = CrateReleaseObservation {
        journal: journal.as_ref(),
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    reject_invalid_metadata_prerequisites(&input);
    assert_eq!(github.writes.get(), 0);
    assert!(inspect_journal_mutation(journal.as_ref(), &identity, 1)?.is_none());
    repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag)?;
    assert_eq!(github.writes.get(), 1);
    assert!(matches!(
        inspect_journal_mutation(journal.as_ref(), &identity, 1)?
            .ok_or("intent")?
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(matches!(
        inspect_journal_mutation(journal.as_ref(), &identity, 2)?
            .ok_or("observed")?
            .phase,
        JournalMutationPhase::Observed { .. }
    ));
    let release = CrateReleaseObservation {
        sequence: 3,
        ..input
    };
    repair_verified_metadata(&release, "demo", JournalMutationKind::CreateRelease)?;
    assert_eq!(github.writes.get(), 2);
    assert!(matches!(
        inspect_journal_mutation(journal.as_ref(), &identity, 3)?
            .ok_or("intent")?
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(matches!(
        inspect_journal_mutation(journal.as_ref(), &identity, 4)?
            .ok_or("observed")?
            .phase,
        JournalMutationPhase::Observed { .. }
    ));
    Ok(())
}

#[test]
fn ambiguous_metadata_write_stops_with_durable_unresolved_intent()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::{
        JournalMutationKind, JournalMutationPhase, inspect_journal_mutation,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.release_variant.set(4);
    github.write_error.set(3);
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    assert!(repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag).is_err());
    assert_eq!(github.writes.get(), 1);
    assert!(matches!(
        inspect_journal_mutation(&journal, &identity, 1)?
            .ok_or("intent")?
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    assert!(repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag).is_err());
    assert_eq!(github.writes.get(), 1);
    Ok(())
}

#[test]
fn metadata_repair_stops_if_approval_changes_after_intent() -> Result<(), Box<dyn std::error::Error>>
{
    use crate::crate_release_controller::{CrateReleaseObservation, repair_verified_metadata};
    use crate::crate_release_journal::{
        JournalMutationKind, JournalMutationPhase, inspect_journal_mutation,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    let github = std::rc::Rc::new(github);
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, github.as_ref(), &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    let observed_github = github.clone();
    *journal.after_ref_created.borrow_mut() = Some(Box::new(move |reference| {
        if reference.ends_with("operation-1") {
            observed_github.approved.set(false);
        }
    }));
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: github.as_ref(),
        registry: &registry,
        source: root,
        sequence: 1,
    };
    assert!(repair_verified_metadata(&input, "demo", JournalMutationKind::CreateTag).is_err());
    assert_eq!(github.writes.get(), 0);
    assert!(matches!(
        inspect_journal_mutation(&journal, &identity, 1)?
            .ok_or("intent")?
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    Ok(())
}

#[test]
fn workspace_receipt_rederives_ordered_source_before_publishing()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, intent, request, github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let receipt = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert_eq!(receipt.packages.len(), 2);
    verify_crate_release_admission(&receipt, &intent, &request, &github, &registry, root)?;
    let mut reordered = receipt.clone();
    reordered.packages.swap(0, 1);
    assert!(
        verify_crate_release_admission(&reordered, &intent, &request, &github, &registry, root)
            .is_err()
    );
    let mut missing = receipt.clone();
    missing.packages.pop();
    assert!(
        verify_crate_release_admission(&missing, &intent, &request, &github, &registry, root)
            .is_err()
    );
    let mut wrong = request.clone();
    wrong.source_sha = "d".repeat(40);
    assert!(
        verify_crate_release_admission(&receipt, &intent, &wrong, &github, &registry, root)
            .is_err()
    );
    fs::write(
        root.join("b/Cargo.toml"),
        "[package]\nname='forged'\nversion='0.1.1'\n",
    )?;
    assert!(
        verify_crate_release_admission(&receipt, &intent, &request, &github, &registry, root)
            .is_err()
    );
    Ok(())
}

#[test]
fn serial_coordinator_records_upload_and_metadata_before_reconciling()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    use crate::crate_release_journal::{
        JournalMutationPhase, inspect_journal_mutation, record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    github.release_variant.set(4);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    let archive = fixture_archive(root)?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let calls = Cell::new(0);
    assert_semver_failure_writes_nothing(&input)?;
    let result = coordinate_release(
        &input,
        |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
        |permit| {
            assert_eq!(permit.package, "demo");
            assert!(matches!(
                inspect_journal_mutation(&journal, &identity, 1)?
                    .expect("missing")
                    .phase,
                JournalMutationPhase::Intended
            ));
            assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
            calls.set(calls.get() + 1);
            *registry.archive.borrow_mut() = archive.clone();
            registry.absent.set(false);
            Ok(UploadAttempt::Uncertain)
        },
        || {},
    )?;
    assert_eq!(calls.get(), 1);
    assert_eq!(github.writes.get(), 2);
    assert_eq!(result.packages.len(), 1);
    assert_eq!(result.history.len(), 6);
    let completed = crate::crate_release_controller::reconciled_completion_packages(&result)?;
    assert_eq!(
        completed[0].owners,
        journal_existing_owners(&plan.packages[0].owners)
    );
    assert_eq!(
        completed[0].operations,
        vec![crate::crate_release_journal::JournalOperation::ObservedPresent]
    );
    assert_eq!(
        completed[0].registry_checksum,
        completed[0].downloaded_checksum
    );
    assert_eq!(completed[0].release_id, 12);
    assert!(
        completed[0]
            .files
            .iter()
            .any(|file| file.path == "src/lib.rs")
    );
    check_completion_retry(&input)?;
    Ok(())
}

fn assert_semver_failure_writes_nothing(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::coordinate_release;
    let before = input.journal.write_calls.get();
    let missing_checker = tempfile::tempdir()?;
    assert!(
        crate::crate_release_semver::verify_before_upload(
            input.source,
            input.admission,
            input.registry,
            missing_checker.path(),
            "demo",
        )
        .is_err()
    );
    assert!(
        coordinate_release(
            input,
            |_| Err(OrchestratorError::Contract {
                problem: "semver_failed".into()
            }),
            |_| panic!("failed semver must not upload"),
            || {},
        )
        .is_err()
    );
    assert_eq!(input.journal.write_calls.get(), before);
    assert_eq!(input.github.writes.get(), 0);
    Ok(())
}

fn check_completion_retry(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::coordinate_release;
    let mut wrong_pins = input.journal_receipt.expected_tools.clone();
    wrong_pins.rust_manifest_sha256 = "0".repeat(64);
    assert!(
        crate::crate_release_journal::record_reconciled_completion(
            input,
            crate::package_release_signer::fixture_measured_tools(wrong_pins)
        )
        .is_err()
    );
    assert!(
        crate::crate_release_journal::inspect_journal(
            input.journal,
            input.identity,
            crate::crate_release_journal::JournalStage::Completed
        )?
        .is_none()
    );
    input.github.approved.set(false);
    assert!(
        crate::crate_release_journal::record_reconciled_completion(
            input,
            crate::package_release_signer::fixture_measured_tools(
                input.journal_receipt.expected_tools.clone()
            )
        )
        .is_err()
    );
    input.github.approved.set(true);
    reject_bad_completion_evidence(input)?;
    crate::crate_release_journal::record_reconciled_completion(
        input,
        crate::package_release_signer::fixture_measured_tools(
            input.journal_receipt.expected_tools.clone(),
        ),
    )?;
    let completion = crate::crate_release_journal::inspect_journal(
        input.journal,
        input.identity,
        crate::crate_release_journal::JournalStage::Completed,
    )?
    .expect("completion");
    assert_eq!(
        completion.completion.expect("result").packages[0].operations,
        vec![crate::crate_release_journal::JournalOperation::ObservedPresent]
    );
    let journal_writes = input.journal.write_calls.get();
    let retried = coordinate_release(
        input,
        |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
        |_| panic!("completed run must not upload"),
        || {},
    )?;
    assert_eq!(retried.history.len(), 6);
    crate::crate_release_journal::record_reconciled_completion(
        input,
        crate::package_release_signer::fixture_measured_tools(
            input.journal_receipt.expected_tools.clone(),
        ),
    )?;
    assert_eq!(input.github.writes.get(), 2);
    assert_eq!(input.journal.write_calls.get(), journal_writes);
    check_stale_completed_retry(input, journal_writes);
    Ok(())
}

fn check_stale_completed_retry(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    journal_writes: usize,
) {
    use crate::crate_release_controller::coordinate_release;
    input.github.owner_match.set(false);
    assert!(
        coordinate_release(
            input,
            |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
            |_| panic!("completed run must not upload"),
            || {}
        )
        .is_err()
    );
    input.github.owner_match.set(true);
    let mut stale_pins = input.journal_receipt.expected_tools.clone();
    stale_pins.release_plz_sha256 = "0".repeat(64);
    assert!(
        crate::crate_release_journal::record_reconciled_completion(
            input,
            crate::package_release_signer::fixture_measured_tools(stale_pins)
        )
        .is_err()
    );
    assert_eq!(input.journal.write_calls.get(), journal_writes);
    assert_eq!(input.github.writes.get(), 2);
}

fn reject_bad_completion_evidence(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    let try_complete = || {
        crate::crate_release_journal::record_reconciled_completion(
            input,
            crate::package_release_signer::fixture_measured_tools(
                input.journal_receipt.expected_tools.clone(),
            ),
        )
    };
    let archive = input.registry.archive.borrow().clone();
    *input.registry.archive.borrow_mut() = vec![0];
    assert!(try_complete().is_err());
    *input.registry.archive.borrow_mut() = archive;
    input.github.owner_match.set(false);
    assert!(try_complete().is_err());
    input.github.owner_match.set(true);
    input.github.tag_wrong.set(true);
    assert!(try_complete().is_err());
    input.github.tag_wrong.set(false);
    input.github.release_variant.set(5);
    assert!(try_complete().is_err());
    input.github.release_variant.set(0);
    let path = input.source.join("src/lib.rs");
    let bytes = fs::read(&path)?;
    fs::write(&path, b"modified source")?;
    assert!(try_complete().is_err());
    fs::write(&path, bytes)?;
    assert!(
        crate::crate_release_journal::inspect_journal(
            input.journal,
            input.identity,
            crate::crate_release_journal::JournalStage::Completed
        )?
        .is_none()
    );
    Ok(())
}

#[test]
fn serial_coordinator_leaves_uncertain_upload_pending_when_not_visible()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    use crate::crate_release_journal::{
        JournalMutationPhase, inspect_journal_mutation, record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let calls = Cell::new(0);
    assert!(
        coordinate_release(
            &input,
            |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
            |_| {
                calls.set(calls.get() + 1);
                Ok(UploadAttempt::Uncertain)
            },
            || {}
        )
        .is_err()
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(github.writes.get(), 0);
    assert!(matches!(
        inspect_journal_mutation(&journal, &identity, 1)?
            .expect("intent")
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    Ok(())
}

#[test]
fn serial_coordinator_recovers_pending_visible_upload_without_uploading_again()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    github.release_variant.set(4);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let result = coordinate_release(
        &input,
        |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
        |_| panic!("must not upload an existing version"),
        || {},
    )?;
    assert_eq!(result.history.len(), 6);
    assert_eq!(github.writes.get(), 2);
    Ok(())
}

#[test]
fn mixed_workspace_admission_retains_publish_and_repair() -> Result<(), Box<dyn std::error::Error>>
{
    let (temp, intent, request, github, registry) = make_workspace_order_fixture()?;
    let initial = prepare_crate_release(&intent, &request, &github, &registry, temp.path())?;
    let archive_dir = tempfile::tempdir()?;
    *registry.archive.borrow_mut() = super::package_source(
        temp.path(),
        &intent,
        &initial.packages[0].source,
        archive_dir.path(),
        &request.source_sha,
        false,
    )?;
    registry.visible.set(true);
    let mixed = prepare_crate_release(&intent, &request, &github, &registry, temp.path())?;
    assert!(matches!(
        mixed.packages[0].action,
        CrateReleaseAction::RepairTag { .. }
    ));
    assert_eq!(mixed.packages[1].action, CrateReleaseAction::Publish);
    assert_eq!(crate::crate_release_runtime::release_mode(&mixed), "mixed");
    Ok(())
}

struct MixedGithub {
    inner: WorkspaceGithub,
    tags: [Cell<bool>; 2],
    releases: [Cell<bool>; 2],
    writes: RefCell<Vec<String>>,
}
impl GitHubAdmissionRead for MixedGithub {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.inner.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        for (i, suffix) in ["a", "b"].iter().enumerate() {
            let tag = format!("velnor_fixture_order_20261104_{suffix}-v0.1.1");
            if route.contains(&tag) {
                if route.contains("/releases/tags/") {
                    return Ok(self.releases[i].get().then(|| json!({"id":12+i,"tag_name":tag,"draft":false,"html_url":format!("https://github.com/example/demo/releases/tag/{tag}")})));
                }
                return Ok(self.tags[i].get().then(|| json!({"ref":format!("refs/tags/{tag}"),"object":{"type":"commit","sha":self.inner.0.sha}})));
            }
        }
        self.inner.get_optional(route)
    }
}
impl CrateReleaseRepairWrite for MixedGithub {
    fn create_tag(&self, _: &str, tag: &str, _: &str) -> Result<(), OrchestratorError> {
        let i = usize::from(tag.contains("_b-v"));
        self.writes.borrow_mut().push(format!("tag-{i}"));
        self.tags[i].set(true);
        Ok(())
    }
    fn create_release(&self, _: &str, tag: &str, _: &str) -> Result<(), OrchestratorError> {
        let i = usize::from(tag.contains("_b-v"));
        self.writes.borrow_mut().push(format!("release-{i}"));
        self.releases[i].set(true);
        Ok(())
    }
}
struct MixedRegistry {
    first: WorkspaceRegistry,
    second: RefCell<Option<Vec<u8>>>,
}
impl CrateRegistryRead for MixedRegistry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        let name = "velnor_fixture_order_20261104_b";
        if path.contains(name) && self.second.borrow().is_some() {
            let archive = self.second.borrow();
            let bytes = archive.as_ref().expect("present");
            if path == format!("/crates/{name}/{name}-0.1.1.crate") {
                return Ok(CrateRegistryResponse {
                    status: 200,
                    body: bytes.clone(),
                });
            }
            if path == format!("/api/v1/crates/{name}/0.1.1") {
                return Ok(CrateRegistryResponse { status: 200, body: json!({"version":{"id":4,"crate":name,"num":"0.1.1","yanked":false,"checksum":test_hex(&Sha256::digest(bytes)),"features":{"pty":[],"base":[],"default":["base"]}}}).to_string().into_bytes() });
            }
            if path == format!("/api/v1/crates/{name}") {
                return Ok(CrateRegistryResponse { status: 200, body: json!({"crate":{"id":name,"name":name,"versions":[3,4]},"versions":[{"id":3,"crate":name,"num":"0.1.0","yanked":false},{"id":4,"crate":name,"num":"0.1.1","yanked":false}]}).to_string().into_bytes() });
            }
        }
        self.first.get(path)
    }
}

#[test]
fn mixed_workspace_coordinator_repairs_predecessor_before_upload()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    use crate::crate_release_journal::record_source_bound_start;
    let (temp, intent, request, original_github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let original = prepare_crate_release(&intent, &request, &original_github, &registry, root)?;
    let archive_dir = tempfile::tempdir()?;
    let first_archive = super::package_source(
        root,
        &intent,
        &original.packages[0].source,
        archive_dir.path(),
        &request.source_sha,
        false,
    )?;
    *registry.archive.borrow_mut() = first_archive.clone();
    registry.visible.set(true);
    let github = MixedGithub {
        inner: original_github,
        tags: [Cell::new(false), Cell::new(false)],
        releases: [Cell::new(false), Cell::new(false)],
        writes: RefCell::new(Vec::new()),
    };
    let registry = MixedRegistry {
        first: registry,
        second: RefCell::new(None),
    };
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert_eq!(
        crate::crate_release_runtime::release_mode(&admission),
        "mixed"
    );
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    set_workspace_journal_packages(&mut plan, &admission);
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    let server = SparseFixture::start("velnor_fixture_order_20261104_a", first_archive)?;
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    let waits = Cell::new(0);
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let result = coordinate_release(
        &input,
        |name| mixed_verified_semver(&admission, &github, &server, name),
        |permit| {
            assert_eq!(permit.package, "velnor_fixture_order_20261104_b");
            assert_eq!(&*github.writes.borrow(), &["tag-0", "release-0"]);
            let target = tempfile::tempdir().map_err(|_| OrchestratorError::Contract {
                problem: "fixture".into(),
            })?;
            let archive = super::package_source(
                root,
                &intent,
                &admission.packages[1].source,
                target.path(),
                &request.source_sha,
                false,
            )?;
            *registry.second.borrow_mut() = Some(archive);
            Ok(UploadAttempt::ExitedSuccess)
        },
        || propagate_sparse_index(&registry, &server, &waits),
    );
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    assert_eq!(waits.get(), 1);
    check_mixed_workspace_result(&github, result?.packages.len());
    Ok(())
}

fn mixed_verified_semver(
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    github: &MixedGithub,
    server: &SparseFixture,
    name: &str,
) -> Result<crate::crate_release_semver::VerifiedSemver, OrchestratorError> {
    assert_eq!(name, "velnor_fixture_order_20261104_b");
    assert_eq!(&*github.writes.borrow(), &["tag-0", "release-0"]);
    assert!(
        server
            .index_visible
            .load(std::sync::atomic::Ordering::SeqCst)
    );
    crate::crate_release_semver::fixture_verified_semver(admission, name)
}

fn propagate_sparse_index(registry: &MixedRegistry, server: &SparseFixture, waits: &Cell<u32>) {
    waits.set(waits.get() + 1);
    registry.first.index_visible.set(true);
    server
        .index_visible
        .store(true, std::sync::atomic::Ordering::SeqCst);
}

fn check_mixed_workspace_result(github: &MixedGithub, packages: usize) {
    assert_eq!(packages, 2);
    assert_eq!(
        &*github.writes.borrow(),
        &["tag-0", "release-0", "tag-1", "release-1"]
    );
}

fn set_workspace_journal_packages(
    plan: &mut crate::crate_release_journal::JournalReceipt,
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) {
    use crate::crate_release_journal::{JournalOwner, JournalOwnerKind, JournalPackage};
    plan.semver_evidence = admission.semver_evidence.clone();
    plan.packages = admission
        .packages
        .iter()
        .map(|p| JournalPackage {
            name: p.source.name.clone(),
            version: p.source.version.clone(),
            manifest: p.source.manifest.clone(),
            tag: p.tag.clone(),
            owners: crate::crate_release_journal::JournalOwnership::Existing {
                owners: vec![JournalOwner {
                    id: 370_532,
                    login: "owner".into(),
                    kind: JournalOwnerKind::User,
                }],
            },
        })
        .collect();
}

#[test]
fn serial_coordinator_recovers_pending_visible_tag_without_recreating()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
        record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    github.release_variant.set(4);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &JournalIntendedMutation {
            semver_evidence: None,
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::CreateTag,
        },
    )?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    github.tag_exists.set(true);
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let result = coordinate_release(
        &input,
        |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
        |_| panic!("must not upload an existing version"),
        || {},
    )?;
    assert_eq!(result.history.len(), 4);
    assert_eq!(github.writes.get(), 1);
    Ok(())
}

#[test]
fn admitted_start_binds_previous_workflow_commit_and_reobserves_prior_upload()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{record_journal_intent, record_source_bound_start};
    use crate::package_release_signer::{
        AdmittedStart, fixture_attested_generator, record_admitted_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let workflow = tempfile::tempdir()?;
    initialize_history_repo(workflow.path())?;
    let (mut intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let mut previous_intent = intent.clone();
    let older_workflow_sha =
        commit_history_intent(workflow.path(), &previous_intent, "previous workflow")?;
    let original = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (_, mut older) = fixture_journal_plan(root, &previous_intent, &request, &original)?;
    older.identity.run_id = 10;
    older.identity.workflow_sha = older_workflow_sha.clone();
    older.workflow_run_url = "https://github.com/example/demo/actions/runs/10".into();
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &older.identity, &older, root)?;
    record_journal_intent(
        &journal,
        &older.identity,
        &older,
        &fixture_upload_intention(),
    )?;
    previous_intent.journal_predecessor =
        Some(velnor_actions_contract::config::CrateReleasePredecessor {
            run_id: 10,
            run_attempt: 1,
            workflow_sha: older_workflow_sha.clone(),
        });
    let previous_workflow_sha =
        commit_history_intent(workflow.path(), &previous_intent, "prior workflow")?;
    let (_, mut previous) = fixture_journal_plan(root, &previous_intent, &request, &original)?;
    previous.identity.run_id = 11;
    previous.identity.workflow_sha = previous_workflow_sha.clone();
    previous.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    previous.predecessor = Some(older.identity.clone());
    record_source_bound_start(&journal, &previous.identity, &previous, root)?;
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: previous_workflow_sha.clone(),
    });
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let generator = fixture_attested_generator();
    let auth = HistoricalAuthority {
        base: &github,
        workflow: previous_workflow_sha.clone(),
        older: Some(older_workflow_sha),
        event: "workflow_dispatch",
        defect: Cell::new(0),
    };
    let input = AdmittedStart {
        journal: &journal,
        github: &auth,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: root,
        workflow_root: workflow.path(),
        generator: &generator,
    };
    fs::write(
        workflow.path().join(".github/velnor-release-intent.toml"),
        "untrusted dirty file",
    )?;
    record_admitted_start(&input)?;
    assert_two_link_recovery(&input, &previous)?;
    Ok(())
}

fn initialize_history_repo(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    git(path, &["init", "-q"])?;
    git(path, &["config", "user.name", "Alexey Zhokhov"])?;
    git(path, &["config", "user.email", "alexey@zhokhov.com"])?;
    git(path, &["config", "commit.gpgsign", "false"])?;
    Ok(())
}

fn commit_history_intent(
    path: &Path,
    intent: &CrateReleaseIntent,
    message: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    fs::create_dir_all(path.join(".github"))?;
    fs::write(
        path.join(".github/velnor-release-intent.toml"),
        toml::to_string(intent)?,
    )?;
    git(path, &["add", "-A"])?;
    git(path, &["commit", "-q", "-s", "-m", message])?;
    git(path, &["rev-parse", "HEAD"])
}

fn assert_two_link_recovery(
    input: &crate::package_release_signer::AdmittedStart<
        '_,
        JournalRegistryFake,
        HistoricalAuthority<'_>,
        Registry,
    >,
    previous: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let identity = crate::crate_release_journal::JournalIdentity {
        repository: input.request.repository.clone(),
        workflow_sha: input.request.workflow_sha.clone(),
        source_sha: input.request.source_sha.clone(),
        run_id: input.request.run_id,
        run_attempt: input.request.run_attempt,
    };
    let started =
        inspect_journal(input.journal, &identity, JournalStage::Started)?.ok_or("start")?;
    assert_eq!(started.receipt.predecessor, Some(previous.identity.clone()));
    let observation = CrateReleaseObservation {
        journal: input.journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission: input.admission,
        intent: input.intent,
        request: input.request,
        github: input.github,
        registry: input.registry,
        source: input.source,
        sequence: 1,
    };
    let writes = input.journal.write_calls.get();
    let attempted = Cell::new(false);
    assert!(
        coordinate_release(
            &observation,
            |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
            |_| {
                attempted.set(true);
                Err(OrchestratorError::Contract {
                    problem: "unexpected upload".into(),
                })
            },
            || {}
        )
        .is_err()
    );
    assert!(!attempted.get());
    assert_eq!(input.journal.write_calls.get(), writes);
    assert_eq!(input.github.base.writes.get(), 0);
    *input.registry.archive.borrow_mut() = fixture_archive(input.source)?;
    input.registry.absent.set(false);
    input.github.base.release_variant.set(4);
    let result = coordinate_release(
        &observation,
        |name| crate::crate_release_semver::fixture_verified_semver(input.admission, name),
        |_| {
            attempted.set(true);
            Err(OrchestratorError::Contract {
                problem: "unexpected upload".into(),
            })
        },
        || {},
    )?;
    assert!(!attempted.get());
    assert_eq!(result.packages.len(), 1);
    assert_eq!(input.github.base.writes.get(), 2);
    Ok(())
}

#[test]
fn admitted_start_rejects_forged_prior_intent_digest() -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{record_journal_intent, record_source_bound_start};
    use crate::package_release_signer::{
        AdmittedStart, fixture_attested_generator, record_admitted_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let workflow = tempfile::tempdir()?;
    git(workflow.path(), &["init", "-q"])?;
    git(workflow.path(), &["config", "user.name", "Alexey Zhokhov"])?;
    git(
        workflow.path(),
        &["config", "user.email", "alexey@zhokhov.com"],
    )?;
    git(workflow.path(), &["config", "commit.gpgsign", "false"])?;
    let (mut intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let mut previous_intent = intent.clone();
    previous_intent.recovery = None;
    fs::create_dir_all(workflow.path().join(".github"))?;
    fs::write(
        workflow.path().join(".github/velnor-release-intent.toml"),
        toml::to_string(&previous_intent)?,
    )?;
    git(workflow.path(), &["add", "-A"])?;
    git(
        workflow.path(),
        &["commit", "-q", "-s", "-m", "previous workflow"],
    )?;
    let previous_workflow_sha = git(workflow.path(), &["rev-parse", "HEAD"])?;
    let original = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (_, mut previous) = fixture_journal_plan(root, &previous_intent, &request, &original)?;
    previous.identity.run_id = 11;
    previous.identity.workflow_sha = previous_workflow_sha.clone();
    previous.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    let journal = JournalRegistryFake::default();
    previous.intent_sha256 = "d".repeat(64);
    record_source_bound_start(&journal, &previous.identity, &previous, root)?;
    record_journal_intent(
        &journal,
        &previous.identity,
        &previous,
        &fixture_upload_intention(),
    )?;
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: previous_workflow_sha,
    });
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let generator = fixture_attested_generator();
    let input = AdmittedStart {
        journal: &journal,
        github: &github,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: root,
        workflow_root: workflow.path(),
        generator: &generator,
    };
    fs::write(
        workflow.path().join(".github/velnor-release-intent.toml"),
        "untrusted dirty file",
    )?;
    let before = journal.write_calls.get();
    assert!(
        record_admitted_start(&input).is_err(),
        "wrong prior intent digest must reject before writes"
    );
    assert_eq!(journal.write_calls.get(), before);
    Ok(())
}

fn fixture_upload_intention() -> crate::crate_release_journal::JournalIntendedMutation {
    crate::crate_release_journal::JournalIntendedMutation {
        semver_evidence: Some(fixture_no_baseline()),
        sequence: 1,
        package: "demo".into(),
        version: "0.1.1".into(),
        kind: crate::crate_release_journal::JournalMutationKind::Upload,
    }
}

struct HistoricalAuthority<'a> {
    base: &'a Github,
    workflow: String,
    older: Option<String>,
    event: &'static str,
    defect: Cell<u8>,
}

impl CrateReleaseRepairWrite for HistoricalAuthority<'_> {
    fn create_tag(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        self.base.create_tag(repository, tag, source_sha)
    }
    fn create_release(
        &self,
        repository: &str,
        tag: &str,
        source_sha: &str,
    ) -> Result<(), OrchestratorError> {
        self.base.create_release(repository, tag, source_sha)
    }
}

impl GitHubAdmissionRead for HistoricalAuthority<'_> {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        let defect = self.defect.get();
        let source = &self.base.sha;
        let old_run = route.contains("/actions/runs/10");
        let run_id = if old_run { 10 } else { 11 };
        let workflow = if old_run {
            self.older.as_ref().unwrap_or(&self.workflow)
        } else {
            &self.workflow
        };
        let check_sha = self
            .older
            .as_ref()
            .filter(|sha| route.contains(sha.as_str()))
            .unwrap_or(&self.workflow);
        let run = || json!({"id":if defect == 1 { 99 } else { run_id },"run_attempt":if defect == 2 { 2 } else { 1 },"status":"completed","head_sha":if defect == 3 { "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee" } else { workflow },"head_branch":"main","event":if defect == 4 { "schedule" } else { self.event },"path":if defect == 5 { ".github/workflows/other.yml" } else { ".github/workflows/release.yml" },"repository":{"id":if defect == 6 { 99 } else { 22 },"full_name":"example/demo"},"head_repository":{"id":22,"full_name":"example/demo"}});
        if route.ends_with("/actions/runs/11/attempts/1")
            || route.ends_with("/actions/runs/11")
            || route.ends_with("/actions/runs/10/attempts/1")
            || route.ends_with("/actions/runs/10")
        {
            return Ok(run());
        }
        if route.ends_with("/actions/runs/11/approvals")
            || route.ends_with("/actions/runs/10/approvals")
        {
            return Ok(if defect == 7 {
                json!([])
            } else {
                json!([{"state":"approved","user":{"type":"User","id":if defect == 8 { 1 } else { 139_017 }},"environments":[{"id":9,"name":"release"}],"comment":format!("I reviewed the complete release policy at {} and authorize source {} for run {}.", workflow, source, run_id)}])
            });
        }
        if (route.contains(&self.workflow)
            || self.older.as_ref().is_some_and(|sha| route.contains(sha)))
            && route.contains("check-runs?")
        {
            return Ok(
                json!({"total_count":1,"check_runs":[{"id":1,"name":"Required","head_sha":check_sha,"check_suite":{"id":123},"app":{"id":15368,"slug":"github-actions"},"status":"completed","conclusion":"success","started_at":"2026-11-03T15:40:00Z"}]}),
            );
        }
        if (route.contains(&self.workflow)
            || self.older.as_ref().is_some_and(|sha| route.contains(sha)))
            && route.contains("/actions/runs?")
        {
            return Ok(
                json!({"total_count":1,"workflow_runs":[{"check_suite_id":123,"head_sha":check_sha,"path":".github/workflows/ci.yml","status":"completed","conclusion":"success"}]}),
            );
        }
        if route.contains("/commits/") && route.contains("/pulls?") {
            return Ok(
                json!([{"number":7,"merged_at":"2026-11-03T15:38:50Z","merge_commit_sha":source,"user":{"login":"author"},"base":{"ref":"main","repo":{"full_name":"example/demo"}},"head":{"ref":"release-plz-test","sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","repo":{"full_name":"example/demo"}}}]),
            );
        }
        if route.contains("/git/commits/") {
            let sha = route.rsplit('/').next().unwrap_or_default();
            return Ok(
                json!({"sha":sha,"tree":{"sha":"cccccccccccccccccccccccccccccccccccccccc"}}),
            );
        }
        if route.contains("/pulls/7/reviews?") {
            return Ok(if defect == 9 {
                json!([])
            } else {
                json!([{"id":1,"user":{"login":"reviewer","id":55,"type":"User"},"author_association":"MEMBER","state":"APPROVED","commit_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","submitted_at":"2026-11-03T15:00:00Z"}])
            });
        }
        if route.ends_with("/collaborators/reviewer/permission") {
            return Ok(json!({"user":{"id":55,"type":"User"},"permission":"write"}));
        }
        self.base.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.base.get_optional(route)
    }
}

#[test]
fn previous_push_or_dispatch_requires_original_historical_authority()
-> Result<(), Box<dyn std::error::Error>> {
    for event in ["push", "workflow_dispatch"] {
        exercise_historical_authority(event, 0)?;
    }
    exercise_historical_authority("workflow_dispatch", 1)?;
    exercise_historical_authority("workflow_dispatch", 2)?;
    Ok(())
}

struct HistoricalFixture {
    temp: tempfile::TempDir,
    workflow: tempfile::TempDir,
    intent: CrateReleaseIntent,
    request: GitHubAdmissionRequest,
    github: Github,
    registry: Registry,
    previous_intent: CrateReleaseIntent,
    prior_sha: String,
}

fn historical_fixture(
    event: &'static str,
    bad_recovery: u8,
) -> Result<HistoricalFixture, Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let source = temp.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(source)?;
    github.approved.set(true);
    let mut previous_intent = intent.clone();
    if bad_recovery == 1 {
        previous_intent.recovery = None;
    }
    if bad_recovery == 2 {
        previous_intent
            .recovery
            .as_mut()
            .ok_or("recovery")?
            .source_sha = "e".repeat(40);
    }
    if event == "push" {
        std::fs::create_dir_all(source.join(".github"))?;
        fs::write(
            source.join(".github/velnor-release-intent.toml"),
            toml::to_string(&previous_intent)?,
        )?;
        git(source, &["add", "-A"])?;
        git(source, &["commit", "-q", "-s", "-m", "prior release"])?;
        let sha = git(source, &["rev-parse", "HEAD"])?;
        request.source_sha.clone_from(&sha);
        request.recovery_sha = Some(sha.clone());
        intent
            .recovery
            .as_mut()
            .ok_or("recovery")?
            .source_sha
            .clone_from(&sha);
        github.sha = sha;
    }
    let workflow = tempfile::tempdir()?;
    let output = Command::new("git")
        .args(["clone", "-q", "--no-hardlinks"])
        .arg(source)
        .arg(workflow.path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if event == "workflow_dispatch" {
        git(workflow.path(), &["config", "user.name", "Fixture"])?;
        git(
            workflow.path(),
            &["config", "user.email", "fixture@example.invalid"],
        )?;
        git(workflow.path(), &["config", "commit.gpgsign", "false"])?;
        fs::create_dir_all(workflow.path().join(".github"))?;
        fs::write(
            workflow.path().join(".github/velnor-release-intent.toml"),
            toml::to_string(&previous_intent)?,
        )?;
        git(workflow.path(), &["add", "-A"])?;
        git(
            workflow.path(),
            &["commit", "-q", "-s", "-m", "prior dispatch"],
        )?;
    }
    let prior_sha = git(workflow.path(), &["rev-parse", "HEAD"])?;
    assert_eq!(prior_sha == request.source_sha, event == "push");
    Ok(HistoricalFixture {
        temp,
        workflow,
        intent,
        request,
        github,
        registry,
        previous_intent,
        prior_sha,
    })
}

fn exercise_historical_authority(
    event: &'static str,
    bad_recovery: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
        record_source_bound_start,
    };
    use crate::package_release_signer::{AdmittedStart, fixture_attested_generator};
    let HistoricalFixture {
        temp,
        workflow,
        mut intent,
        request,
        github,
        registry,
        previous_intent,
        prior_sha,
    } = historical_fixture(event, bad_recovery)?;
    let source = temp.path();
    let observed = prepare_crate_release(&intent, &request, &github, &registry, source)?;
    let (_, mut prior) = fixture_journal_plan(source, &previous_intent, &request, &observed)?;
    prior.identity.run_id = 11;
    prior.identity.workflow_sha.clone_from(&prior_sha);
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &prior.identity, &prior, source)?;
    record_journal_intent(
        &journal,
        &prior.identity,
        &prior,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: prior_sha.clone(),
    });
    let admission = prepare_crate_release(&intent, &request, &github, &registry, source)?;
    let auth = HistoricalAuthority {
        base: &github,
        workflow: prior_sha,
        older: None,
        event,
        defect: Cell::new(0),
    };
    let generator = fixture_attested_generator();
    let input = AdmittedStart {
        journal: &journal,
        github: &auth,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source,
        workflow_root: workflow.path(),
        generator: &generator,
    };
    check_historical_authority(&input, event, bad_recovery)?;
    Ok(())
}

fn check_historical_authority(
    input: &crate::package_release_signer::AdmittedStart<
        '_,
        JournalRegistryFake,
        HistoricalAuthority<'_>,
        Registry,
    >,
    event: &str,
    bad_recovery: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::package_release_signer::record_admitted_start;
    for defect in 1..=if event == "push" { 9 } else { 8 } {
        input.github.defect.set(defect);
        let writes = input.journal.write_calls.get();
        assert!(
            record_admitted_start(input).is_err(),
            "accepted {event} historical defect {defect}"
        );
        assert_eq!(
            input.journal.write_calls.get(),
            writes,
            "wrote journal for defect {defect}"
        );
        assert_eq!(
            input.github.base.writes.get(),
            0,
            "wrote metadata for defect {defect}"
        );
    }
    input.github.defect.set(0);
    if bad_recovery != 0 {
        let writes = input.journal.write_calls.get();
        assert!(
            record_admitted_start(input).is_err(),
            "accepted unapproved historical recovery source"
        );
        assert_eq!(input.journal.write_calls.get(), writes);
        assert_eq!(input.github.base.writes.get(), 0);
    } else {
        record_admitted_start(input)?;
    }
    Ok(())
}

#[test]
fn workflow_lock_uses_event_scoped_recovery_and_committed_policy()
-> Result<(), Box<dyn std::error::Error>> {
    let source = tempfile::tempdir()?;
    let policy = tempfile::tempdir()?;
    let (mut intent, mut request, github, registry) = make_fixture(source.path())?;
    intent.recovery.as_mut().expect("recovery").source_sha = "f".repeat(40);
    request.event = "push".into();
    request.recovery_sha = None;
    let effective = super::effective_release_intent(&intent, &request);
    let plan = crate::crate_release_workspace::crate_release_workspace_plan_with_registry(
        source.path(),
        &request.source_sha,
        &effective,
        &registry,
    )?;
    git(policy.path(), &["init", "-q"])?;
    fs::create_dir_all(policy.path().join(".github/workflows"))?;
    let workflow = policy.path().join(".github/workflows/release.yml");
    fs::write(
        &workflow,
        format!(
            "name: release\nconcurrency:\n  group: {}\n  cancel-in-progress: false\njobs: {{}}\n",
            plan.concurrency_group
        ),
    )?;
    git(policy.path(), &["add", "."])?;
    git(
        policy.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "policy",
        ],
    )?;
    request.workflow_sha = git(policy.path(), &["rev-parse", "HEAD"])?;
    crate::crate_release_runtime::verify_workflow_concurrency(
        policy.path(),
        source.path(),
        &intent,
        &request,
        &registry,
    )?;
    fs::write(source.path().join("src/lib.rs"), "pub fn modified() {}")?;
    assert!(
        crate::crate_release_runtime::verify_workflow_concurrency(
            policy.path(),
            source.path(),
            &intent,
            &request,
            &registry,
        )
        .is_err()
    );
    fs::write(source.path().join("src/lib.rs"), "pub fn demo() {}")?;
    request.event = "workflow_dispatch".into();
    request.recovery_sha = Some("f".repeat(40));
    assert!(
        crate::crate_release_runtime::verify_workflow_concurrency(
            policy.path(),
            source.path(),
            &intent,
            &request,
            &registry
        )
        .is_err()
    );
    assert_eq!(github.writes.get(), 0);
    Ok(())
}

fn journal_existing_owners(
    value: &crate::crate_release_journal::JournalOwnership,
) -> Vec<crate::crate_release_journal::JournalOwner> {
    let crate::crate_release_journal::JournalOwnership::Existing { owners } = value else {
        panic!("expected existing owners")
    };
    owners.clone()
}

#[test]
fn explicitly_approved_new_crate_admission_records_unknown_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    struct Absent;
    impl CrateRegistryRead for Absent {
        fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
            if path.starts_with("/api/v1/crates/demo") {
                Ok(CrateRegistryResponse {
                    status: 404,
                    body: Vec::new(),
                })
            } else {
                Err(OrchestratorError::Contract {
                    problem: "unexpected route".into(),
                })
            }
        }
    }
    let temp = tempfile::tempdir()?;
    let (mut intent, request, github, _) = make_fixture(temp.path())?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.recovery = None;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    github.approved.set(true);
    let receipt = prepare_crate_release(&intent, &request, &github, &Absent, temp.path())?;
    assert_eq!(
        receipt.packages[0].owners,
        crate::crate_release_registry::CrateRegistryOwnership::NewCrate
    );
    assert_eq!(github.writes.get(), 0);
    let mut no_approval = intent.clone();
    no_approval.new_crates = None;
    assert!(prepare_crate_release(&no_approval, &request, &github, &Absent, temp.path()).is_err());
    let mut non_dispatch = request.clone();
    non_dispatch.event = "push".into();
    assert!(prepare_crate_release(&intent, &non_dispatch, &github, &Absent, temp.path()).is_err());
    let mut wrong_version = intent.clone();
    wrong_version
        .new_crates
        .as_mut()
        .ok_or("creation")?
        .packages
        .insert("demo".into(), "0.1.2".into());
    assert!(
        prepare_crate_release(&wrong_version, &request, &github, &Absent, temp.path()).is_err()
    );
    assert_eq!(github.writes.get(), 0);
    Ok(())
}

#[test]
fn new_crate_upload_observation_requires_durable_intent_and_real_reviewer_owner()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        CrateReleaseObservation, record_verified_mutation,
    };
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalAuthentication, JournalIntendedMutation, JournalMutationKind, JournalOwnership,
        record_journal_intent, record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, request, github, base) = make_fixture(root)?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.recovery = None;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    github.approved.set(true);
    let registry = NewRegistry {
        base,
        visible: Cell::new(false),
        owner_changed: Cell::new(false),
    };
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert_eq!(
        admission.packages[0].owners,
        crate::crate_release_registry::CrateRegistryOwnership::NewCrate
    );
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    plan.authentication = JournalAuthentication::BootstrapToken;
    plan.packages[0].owners = JournalOwnership::NewCrate;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    let intended = JournalIntendedMutation {
        sequence: 1,
        package: "demo".into(),
        version: "0.1.1".into(),
        kind: JournalMutationKind::Upload,
        semver_evidence: Some(fixture_no_baseline()),
    };
    let obs = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 2,
    };
    assert!(record_verified_mutation(&obs).is_err());
    record_journal_intent(&journal, &identity, &plan, &intended)?;
    *registry.base.archive.borrow_mut() = fixture_archive(root)?;
    registry.base.absent.set(false);
    registry.visible.set(true);
    github.owner_match.set(false);
    let before = journal.write_calls.get();
    assert!(record_verified_mutation(&obs).is_err());
    assert_eq!(journal.write_calls.get(), before);
    github.owner_match.set(true);
    let metadata = CrateReleaseObservation { sequence: 3, ..obs };
    let before_metadata = github.writes.get();
    assert!(
        crate::crate_release_controller::repair_verified_metadata(
            &metadata,
            "demo",
            JournalMutationKind::CreateTag
        )
        .is_err()
    );
    assert_eq!(github.writes.get(), before_metadata);
    let upload_obs = CrateReleaseObservation {
        sequence: 2,
        ..metadata
    };
    record_verified_mutation(&upload_obs)?;
    verify_recorded_new_crate_owner(&journal, &identity)?;
    verify_new_crate_metadata(&upload_obs, &github, &registry, before_metadata)?;
    Ok(())
}

fn verify_recorded_new_crate_owner(
    journal: &JournalRegistryFake,
    identity: &crate::crate_release_journal::JournalIdentity,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalMutationPhase, JournalMutationResult, inspect_journal_mutation,
    };
    let record = inspect_journal_mutation(journal, identity, 2)?.ok_or("missing")?;
    let JournalMutationPhase::Observed {
        result: JournalMutationResult::Upload { owners, .. },
    } = record.phase
    else {
        return Err("expected upload".into());
    };
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].login, "owner");
    Ok(())
}

fn verify_new_crate_metadata(
    upload_obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        NewRegistry,
    >,
    github: &Github,
    registry: &NewRegistry,
    before_metadata: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::CrateReleaseObservation;
    use crate::crate_release_journal::JournalMutationKind;
    let metadata = CrateReleaseObservation {
        sequence: 3,
        ..*upload_obs
    };
    github.owner_match.set(false);
    assert!(
        crate::crate_release_controller::repair_verified_metadata(
            &metadata,
            "demo",
            JournalMutationKind::CreateTag
        )
        .is_err()
    );
    assert_eq!(github.writes.get(), before_metadata);
    github.owner_match.set(true);
    registry.owner_changed.set(true);
    assert!(
        crate::crate_release_controller::repair_verified_metadata(
            &metadata,
            "demo",
            JournalMutationKind::CreateTag
        )
        .is_err()
    );
    assert_eq!(github.writes.get(), before_metadata);
    registry.owner_changed.set(false);
    github.release_variant.set(4);
    crate::crate_release_controller::repair_verified_metadata(
        &metadata,
        "demo",
        JournalMutationKind::CreateTag,
    )?;
    assert_eq!(github.writes.get(), before_metadata + 1);
    let release_metadata = CrateReleaseObservation {
        sequence: 5,
        ..metadata
    };
    crate::crate_release_controller::repair_verified_metadata(
        &release_metadata,
        "demo",
        JournalMutationKind::CreateRelease,
    )?;
    assert_eq!(github.writes.get(), before_metadata + 2);
    let completion = CrateReleaseObservation {
        sequence: 7,
        ..release_metadata
    };
    let candidate = crate::crate_release_controller::derive_reconciled_candidate(&completion)?;
    assert_eq!(candidate.packages.len(), 1);
    Ok(())
}

#[test]
fn two_new_workspace_crates_require_each_own_recorded_upload()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::CrateReleaseObservation;
    use crate::crate_release_journal::{
        JournalAuthentication, JournalOwnership, inspect_journal_mutation, record_journal_intent,
        record_source_bound_start,
    };
    let (temp, mut intent, mut request, github, registry) = make_workspace_order_fixture()?;
    let root = temp.path();
    let original = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: original
            .packages
            .iter()
            .map(|package| (package.source.name.clone(), package.source.version.clone()))
            .collect(),
    });
    request.event = "workflow_dispatch".into();
    request.recovery_sha = Some(request.source_sha.clone());
    let mut admission = original.clone();
    admission.intent = intent.clone();
    admission.github.request = request.clone();
    admission.packages.iter_mut().for_each(|package| {
        package.owners = crate::crate_release_registry::CrateRegistryOwnership::NewCrate;
    });
    let current = created_workspace_admission(&admission, &original)?;
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    plan.authentication = JournalAuthentication::BootstrapToken;
    set_workspace_journal_packages(&mut plan, &admission);
    for package in &mut plan.packages {
        package.owners = JournalOwnership::NewCrate;
    }
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    let first = &admission.packages[0].source;
    let second = &admission.packages[1].source;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &fixture_creation_intention(1, &first.name, &first.version),
    )?;
    record_first_created_workspace_crate(&journal, &identity, &plan, first)?;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &fixture_creation_intention(3, &second.name, &second.version),
    )?;
    let pending = inspect_journal_mutation(&journal, &identity, 3)?.ok_or("intent")?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 4,
    };
    super::observation::verify_observed_packages_with_upload_intent(
        &input,
        &current,
        Some(&pending),
    )?;
    reject_changed_workspace_owner(&input, &current, &pending)?;
    Ok(())
}

fn reject_changed_workspace_owner(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        WorkspaceGithub,
        WorkspaceRegistry,
    >,
    current: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    pending: &crate::crate_release_journal::JournalMutation,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut wrong = current.clone();
    let crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } =
        &mut wrong.packages[0].owners
    else {
        return Err("owners".into());
    };
    owners[0].id += 1;
    assert!(
        super::observation::verify_observed_packages_with_upload_intent(
            input,
            &wrong,
            Some(pending)
        )
        .is_err()
    );
    Ok(())
}

fn fixture_creation_intention(
    sequence: u32,
    name: &str,
    version: &str,
) -> crate::crate_release_journal::JournalIntendedMutation {
    crate::crate_release_journal::JournalIntendedMutation {
        sequence,
        package: name.into(),
        version: version.into(),
        kind: crate::crate_release_journal::JournalMutationKind::Upload,
        semver_evidence: Some(fixture_no_baseline()),
    }
}

fn created_workspace_admission(
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    original: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<crate::crate_release_controller::CrateReleaseAdmissionReceipt, Box<dyn std::error::Error>>
{
    use crate::crate_release_controller::CrateReleaseAction;
    use crate::crate_release_registry::CrateRegistryReconciliation;
    let mut current = admission.clone();
    for (now, original) in current.packages.iter_mut().zip(&original.packages) {
        now.owners = original.owners.clone();
        let crate::crate_release_registry::CrateRegistryOwnership::Existing { owners } =
            &now.owners
        else {
            return Err("owners".into());
        };
        now.action = CrateReleaseAction::RepairTag {
            registry: CrateRegistryReconciliation {
                checksum: "d".repeat(64),
                downloaded_checksum: "d".repeat(64),
                owners: owners.clone(),
                features: std::collections::BTreeMap::new(),
            },
        };
    }
    Ok(current)
}

fn record_first_created_workspace_crate(
    journal: &JournalRegistryFake,
    identity: &crate::crate_release_journal::JournalIdentity,
    plan: &crate::crate_release_journal::JournalReceipt,
    first: &crate::crate_release_source::CrateReleaseSourcePackage,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalFile, JournalFileKind, JournalMutation, JournalMutationKind, JournalMutationPhase,
        JournalMutationResult, fixture_record_journal_mutation,
    };
    let recorded_owners = vec![crate::crate_release_journal::JournalOwner {
        id: 370_532,
        login: "owner".into(),
        kind: crate::crate_release_journal::JournalOwnerKind::User,
    }];
    fixture_record_journal_mutation(
        journal,
        identity,
        plan,
        &JournalMutation {
            sequence: 2,
            package: first.name.clone(),
            version: first.version.clone(),
            kind: JournalMutationKind::Upload,
            semver_evidence: None,
            phase: JournalMutationPhase::Observed {
                result: JournalMutationResult::Upload {
                    registry_checksum: "d".repeat(64),
                    downloaded_checksum: "d".repeat(64),
                    owners: recorded_owners,
                    features: std::collections::BTreeMap::new(),
                    files: vec![JournalFile {
                        path: "src/lib.rs".into(),
                        sha256: "a".repeat(64),
                        kind: JournalFileKind::Regular { executable: false },
                    }],
                },
            },
        },
    )?;
    Ok(())
}

#[test]
fn first_crate_prior_upload_recovers_only_with_authenticated_intent_and_owner()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalAuthentication, JournalOwnership, record_source_bound_start,
    };
    use crate::package_release_signer::{
        AdmittedStart, fixture_attested_generator, record_admitted_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let workflow = tempfile::tempdir()?;
    initialize_history_repo(workflow.path())?;
    let (mut intent, request, github, base) = make_fixture(root)?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.recovery = None;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    github.approved.set(true);
    let registry = NewRegistry {
        base,
        visible: Cell::new(false),
        owner_changed: Cell::new(false),
    };
    let previous_workflow =
        commit_history_intent(workflow.path(), &intent, "prior creation workflow")?;
    let initial = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (_, mut prior) = fixture_journal_plan(root, &intent, &request, &initial)?;
    prior.identity.run_id = 11;
    prior.identity.workflow_sha = previous_workflow.clone();
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    prior.authentication = JournalAuthentication::BootstrapToken;
    prior.packages[0].owners = JournalOwnership::NewCrate;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &prior.identity, &prior, root)?;
    *registry.base.archive.borrow_mut() = fixture_archive(root)?;
    registry.base.absent.set(false);
    registry.visible.set(true);
    intent.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 11,
        run_attempt: 1,
        workflow_sha: previous_workflow.clone(),
    });
    let auth = HistoricalAuthority {
        base: &github,
        workflow: previous_workflow,
        older: None,
        event: "workflow_dispatch",
        defect: Cell::new(0),
    };
    let mut admission = prepare_crate_release(&intent, &request, &auth, &registry, root)?;
    let pending = crate::crate_release_semver::prior_no_baseline_pending(
        &admission,
        &registry,
        &admission.packages[0],
    )?
    .ok_or("pending")?;
    admission.semver_evidence.insert("demo".into(), pending);
    let generator = fixture_attested_generator();
    let input = AdmittedStart {
        journal: &journal,
        github: &auth,
        registry: &registry,
        intent: &intent,
        request: &request,
        admission: &admission,
        source: root,
        workflow_root: workflow.path(),
        generator: &generator,
    };
    reject_untrusted_prior_creation(&input, &prior)?;
    record_admitted_start(&input)?;
    verify_created_crate_recovers_without_reupload(&input)?;
    Ok(())
}

fn verify_created_crate_recovers_without_reupload(
    input: &crate::package_release_signer::AdmittedStart<
        '_,
        JournalRegistryFake,
        HistoricalAuthority<'_>,
        NewRegistry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let request = input.request;
    let journal = input.journal;
    let admission = input.admission;
    let intent = input.intent;
    let auth = input.github;
    let registry = input.registry;
    let root = input.source;
    let github = auth.base;
    let identity = crate::crate_release_journal::JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let started = inspect_journal(journal, &identity, JournalStage::Started)?.ok_or("start")?;
    let observation = CrateReleaseObservation {
        journal,
        identity: &identity,
        journal_receipt: &started.receipt,
        admission,
        intent,
        request,
        github: auth,
        registry,
        source: root,
        sequence: 1,
    };
    github.release_variant.set(4);
    let attempted = Cell::new(false);
    let result = coordinate_release(
        &observation,
        |name| crate::crate_release_semver::fixture_verified_semver(admission, name),
        |_| {
            attempted.set(true);
            Err(OrchestratorError::Contract {
                problem: "must not reupload".into(),
            })
        },
        || {},
    )?;
    assert!(!attempted.get());
    assert_eq!(result.packages.len(), 1);
    Ok(())
}

fn reject_untrusted_prior_creation(
    input: &crate::package_release_signer::AdmittedStart<
        '_,
        JournalRegistryFake,
        HistoricalAuthority<'_>,
        NewRegistry,
    >,
    prior: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
    };
    use crate::package_release_signer::record_admitted_start;
    let journal = input.journal;
    let auth = input.github;
    let registry = input.registry;
    let github = auth.base;
    let before = journal.write_calls.get();
    assert!(record_admitted_start(input).is_err());
    assert_eq!(journal.write_calls.get(), before);
    record_journal_intent(
        journal,
        &prior.identity,
        prior,
        &JournalIntendedMutation {
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            semver_evidence: Some(fixture_no_baseline()),
        },
    )?;
    auth.defect.set(7);
    let before = journal.write_calls.get();
    assert!(record_admitted_start(input).is_err());
    assert_eq!(journal.write_calls.get(), before);
    auth.defect.set(0);
    github.owner_match.set(false);
    assert!(record_admitted_start(input).is_err());
    assert_eq!(journal.write_calls.get(), before);
    github.owner_match.set(true);
    let original_archive = registry.base.archive.borrow().clone();
    registry.base.archive.borrow_mut().push(0);
    assert!(record_admitted_start(input).is_err());
    assert_eq!(journal.write_calls.get(), before);
    *registry.base.archive.borrow_mut() = original_archive;
    Ok(())
}

#[test]
fn coordinator_completes_first_crate_with_durable_owners() -> Result<(), Box<dyn std::error::Error>>
{
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{
        JournalAuthentication, JournalOwnership, record_source_bound_start,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, request, github, base) = make_fixture(root)?;
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.recovery = None;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: std::collections::BTreeMap::from([("demo".into(), "0.1.1".into())]),
    });
    github.approved.set(true);
    github.release_variant.set(4);
    let registry = NewRegistry {
        base,
        visible: Cell::new(false),
        owner_changed: Cell::new(false),
    };
    *registry.base.archive.borrow_mut() = fixture_archive(root)?;
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    plan.authentication = JournalAuthentication::BootstrapToken;
    plan.packages[0].owners = JournalOwnership::NewCrate;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let uploads = Cell::new(0);
    let result = coordinate_release(
        &input,
        |name| crate::crate_release_semver::fixture_verified_semver(&admission, name),
        |_| {
            uploads.set(uploads.get() + 1);
            registry.base.absent.set(false);
            registry.visible.set(true);
            Ok(super::observation::UploadAttempt::ExitedSuccess)
        },
        || {},
    )?;
    assert_eq!(uploads.get(), 1);
    assert_eq!(github.writes.get(), 2);
    assert_eq!(result.packages.len(), 1);
    let tools = crate::package_release_signer::fixture_measured_tools(plan.expected_tools.clone());
    crate::crate_release_journal::record_reconciled_completion(&input, tools)?;
    let completed = crate::crate_release_journal::inspect_journal(
        &journal,
        &identity,
        crate::crate_release_journal::JournalStage::Completed,
    )?
    .ok_or("missing completion")?
    .completion
    .ok_or("missing completion data")?;
    assert_eq!(completed.packages[0].owners.len(), 1);
    assert_eq!(completed.packages[0].owners[0].id, 370_532);
    assert_eq!(completed.packages[0].owners[0].login, "owner");
    let writes = journal.write_calls.get();
    let tools = crate::package_release_signer::fixture_measured_tools(plan.expected_tools.clone());
    crate::crate_release_journal::record_reconciled_completion(&input, tools)?;
    assert_eq!(journal.write_calls.get(), writes);
    assert_eq!(github.writes.get(), 2);
    reject_changed_first_crate_completion(&input, writes);
    Ok(())
}

fn reject_changed_first_crate_completion(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        NewRegistry,
    >,
    writes: usize,
) {
    input.registry.owner_changed.set(true);
    let tools = crate::package_release_signer::fixture_measured_tools(
        input.journal_receipt.expected_tools.clone(),
    );
    assert!(crate::crate_release_journal::record_reconciled_completion(input, tools).is_err());
    input.registry.owner_changed.set(false);
    input.registry.base.archive.borrow_mut().push(0);
    let tools = crate::package_release_signer::fixture_measured_tools(
        input.journal_receipt.expected_tools.clone(),
    );
    assert!(crate::crate_release_journal::record_reconciled_completion(input, tools).is_err());
    assert_eq!(input.journal.write_calls.get(), writes);
    assert_eq!(input.github.writes.get(), 2);
}

struct CreationWorkspaceGithub(MixedGithub);
impl GitHubAdmissionRead for CreationWorkspaceGithub {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        if route.ends_with("/actions/runs/12") {
            self.0.inner.0.get(route)
        } else {
            self.0.get(route)
        }
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        self.0.get_optional(route)
    }
}
impl CrateReleaseRepairWrite for CreationWorkspaceGithub {
    fn create_tag(&self, repo: &str, tag: &str, sha: &str) -> Result<(), OrchestratorError> {
        self.0.create_tag(repo, tag, sha)
    }
    fn create_release(&self, repo: &str, tag: &str, sha: &str) -> Result<(), OrchestratorError> {
        self.0.create_release(repo, tag, sha)
    }
}
struct CreationWorkspaceRegistry {
    inner: MixedRegistry,
    owner_changed: Cell<bool>,
}
impl CrateRegistryRead for CreationWorkspaceRegistry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        for (index, suffix) in ["a", "b"].iter().enumerate() {
            let name = format!("velnor_fixture_order_20261104_{suffix}");
            let root = format!("/api/v1/crates/{name}");
            let visible = if index == 0 {
                self.inner.first.visible.get()
            } else {
                self.inner.second.borrow().is_some()
            };
            if path == root || path.starts_with(&format!("{root}/")) {
                if !visible {
                    return Ok(CrateRegistryResponse {
                        status: 404,
                        body: vec![],
                    });
                }
                if path == root {
                    let id = if index == 0 { 2 } else { 4 };
                    return Ok(CrateRegistryResponse { status: 200, body: json!({"crate":{"id":name,"name":name,"versions":[id]},"versions":[{"id":id,"crate":name,"num":"0.1.1","yanked":false}]}).to_string().into_bytes() });
                }
                if index == 0 && self.owner_changed.get() && path == format!("{root}/owners") {
                    return Ok(CrateRegistryResponse {
                        status: 200,
                        body: br#"{"users":[{"id":370533,"login":"owner","kind":"user"}]}"#
                            .to_vec(),
                    });
                }
            }
        }
        self.inner.get(path)
    }
}

#[test]
fn coordinator_publishes_two_new_workspace_crates_only_with_recorded_owners()
-> Result<(), Box<dyn std::error::Error>> {
    run_two_new_workspace_crates(false)?;
    run_two_new_workspace_crates(true)
}

fn run_two_new_workspace_crates(change_owner: bool) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::CrateReleaseObservation;
    use crate::crate_release_journal::{
        JournalAuthentication, JournalOwnership, record_source_bound_start,
    };
    let (temp, mut intent, mut request, original_github, first) = make_workspace_order_fixture()?;
    let root = temp.path();
    intent.authentication = velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken;
    intent.new_crates = Some(velnor_actions_contract::config::NewCrateRelease {
        source_sha: request.source_sha.clone(),
        packages: intent
            .packages
            .iter()
            .map(|name| (name.clone(), "0.1.1".into()))
            .collect(),
    });
    request.event = "workflow_dispatch".into();
    request.recovery_sha = Some(request.source_sha.clone());
    let github = CreationWorkspaceGithub(MixedGithub {
        inner: original_github,
        tags: [Cell::new(false), Cell::new(false)],
        releases: [Cell::new(false), Cell::new(false)],
        writes: RefCell::new(Vec::new()),
    });
    let registry = CreationWorkspaceRegistry {
        inner: MixedRegistry {
            first,
            second: RefCell::new(None),
        },
        owner_changed: Cell::new(false),
    };
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    assert!(
        admission
            .packages
            .iter()
            .all(|p| p.owners == crate::crate_release_registry::CrateRegistryOwnership::NewCrate)
    );
    let target = tempfile::tempdir()?;
    let first_archive = super::package_source(
        root,
        &intent,
        &admission.packages[0].source,
        target.path(),
        &request.source_sha,
        false,
    )?;
    let server = SparseFixture::start("velnor_fixture_order_20261104_a", first_archive.clone())?;
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    let (identity, mut plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    plan.authentication = JournalAuthentication::BootstrapToken;
    plan.expected_tools.release_plz_sha256 = velnor_actions_mise::catalog::qualified_release_plz(
        &intent.release_plz_version,
        &intent.runner,
    )
    .ok_or("qualified release-plz")?
    .executable_sha256
    .into();
    set_workspace_journal_packages(&mut plan, &admission);
    for package in &mut plan.packages {
        package.owners = JournalOwnership::NewCrate;
    }
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root)?;
    let input = CrateReleaseObservation {
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    execute_two_new_workspace_crates(&input, &first_archive, &server, change_owner)?;
    Ok(())
}

fn execute_two_new_workspace_crates(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        CreationWorkspaceGithub,
        CreationWorkspaceRegistry,
    >,
    first_archive: &[u8],
    server: &SparseFixture,
    change_owner: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{UploadAttempt, coordinate_release};
    let admission = input.admission;
    let intent = input.intent;
    let request = input.request;
    let registry = input.registry;
    let github = input.github;
    let root = input.source;
    let uploads = RefCell::new(Vec::new());
    let result = coordinate_release(
        input,
        |name| crate::crate_release_semver::fixture_verified_semver(admission, name),
        |permit| {
            let rendered = super::fixture_upload_config(intent, &permit.package)?;
            let config: toml::Value = toml::from_str(&rendered).map_err(|_| err())?;
            let packages = config["package"].as_array().ok_or_else(err)?;
            assert_eq!(packages.len(), 1);
            assert_eq!(packages[0]["name"].as_str(), Some(permit.package.as_str()));
            super::plz::validate_upload(
                input,
                &permit,
                &velnor_actions_mise::command::CratePublishCredential::Bootstrap {
                    cargo_token: "fixture",
                },
            )?;
            uploads.borrow_mut().push(permit.package.clone());
            if permit.package.ends_with("_a") {
                *registry.inner.first.archive.borrow_mut() = first_archive.to_vec();
                registry.inner.first.visible.set(true);
            } else {
                let target = tempfile::tempdir().map_err(|_| OrchestratorError::Contract {
                    problem: "fixture".into(),
                })?;
                let archive = super::package_source(
                    root,
                    intent,
                    &admission.packages[1].source,
                    target.path(),
                    &request.source_sha,
                    false,
                )?;
                *registry.inner.second.borrow_mut() = Some(archive);
            }
            Ok(UploadAttempt::ExitedSuccess)
        },
        || {
            registry.inner.first.index_visible.set(true);
            server
                .index_visible
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if change_owner {
                registry.owner_changed.set(true);
            }
        },
    );
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    if change_owner {
        assert!(result.is_err());
        assert_eq!(&*uploads.borrow(), &["velnor_fixture_order_20261104_a"]);
        assert_eq!(&*github.0.writes.borrow(), &["tag-0", "release-0"]);
    } else {
        assert_eq!(result?.packages.len(), 2);
        assert_eq!(
            &*uploads.borrow(),
            &[
                "velnor_fixture_order_20261104_a",
                "velnor_fixture_order_20261104_b"
            ]
        );
        assert_eq!(
            &*github.0.writes.borrow(),
            &["tag-0", "release-0", "tag-1", "release-1"]
        );
    }
    Ok(())
}

#[cfg(unix)]
#[path = "crate_release_coordinator_rehearsal.rs"]
mod coordinator_rehearsal;
