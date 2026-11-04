// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn test_git() -> &'static crate::crate_release_git::ReleaseGit {
    static GIT: std::sync::OnceLock<crate::crate_release_git::ReleaseGit> =
        std::sync::OnceLock::new();
    GIT.get_or_init(crate::crate_release_git::ReleaseGit::host_fixture)
}

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

#[expect(
    clippy::too_many_arguments,
    reason = "This test helper binds the full package-verification fixture"
)]
fn fixture_verify_release_package<G: GitHubAdmissionRead, R: CrateRegistryRead>(
    receipt: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    github: &G,
    registry: &R,
    source: &Path,
    package: &str,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<crate::crate_release_controller::VerifiedReleasePackage, OrchestratorError> {
    let input = super::PackageVerification {
        receipt,
        intent,
        request,
        github,
        registry,
        source,
        git,
        package,
    };
    super::verify_release_package_with(&input, |observed| {
        super::verify_observed_packages(observed, receipt)
    })
}

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
    let mut receipt =
        raw_prepare_crate_release(intent, request, github, registry, source, test_git())?;
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
    revoke_on_tag_read: Cell<bool>,
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
                json!([{"state":"approved","user":{"type":"User","id":139_017},"environments":[{"id":9,"name":"release"}],"comment":format!("I reviewed the complete release policy at {policy_sha} and source {sha}, and authorize this source for run 12.")}])
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
        if route.contains("/git/ref/tags/") && self.revoke_on_tag_read.replace(false) {
            self.approved.set(false);
        }
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
