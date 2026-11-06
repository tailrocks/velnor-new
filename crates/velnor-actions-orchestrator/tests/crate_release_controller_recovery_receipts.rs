// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, crate_release_github::GitHubAdmissionRead};
use serde_json::{Value, json};
use std::cell::Cell;

use super::{
    Github, Path, Registry, fixture_archive, fs, test_git, velnor_actions_orchestrator,
    verify_crate_release_admission,
};

pub(super) fn check_receipt_denials(
    stage: &crate::crate_release_tool_artifact::ToolchainStageRequest<'_>,
    github: &Github,
    registry: &Registry,
    receipt: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    source: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let reconcile = || {
        velnor_actions_orchestrator::crate_release_tool_artifact::acquire_reconciliation_receipt_artifact(stage, github, registry, github, crate::CrateSource::new(source, test_git(), &crate::UnprivilegedSourceExecution))
    };
    github.tag_exists.set(true);
    reconcile()?;
    fs::remove_file(stage.runner_temp.join("velnor-crate-release-receipt.json"))?;
    github.tag_wrong.set(true);
    assert!(reconcile().is_err());
    github.tag_wrong.set(false);
    registry.inconsistent.set(true);
    assert!(
        !velnor_actions_orchestrator::crate_release_controller::validate_reconciliation_receipt(
            receipt,
            stage.intent,
            stage.github,
            github,
            registry,
            crate::CrateSource::new(source, test_git(), &crate::UnprivilegedSourceExecution)
        )?
    );
    registry.inconsistent.set(false);
    registry.absent.set(true);
    assert!(reconcile().is_err());
    registry.absent.set(false);
    github.owner_match.set(false);
    assert!(reconcile().is_err());
    github.owner_match.set(true);
    registry.absent.set(true);
    github.tag_exists.set(false);
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec_pretty(receipt)?)?;
    assert!(
        crate::crate_release_tool_artifact::fixture_acquire_receipt(
            stage,
            github,
            registry,
            github,
            crate::CrateSource::new(source, test_git(), &crate::UnprivilegedSourceExecution)
        )
        .is_err()
    );
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(receipt)?)?;
    github.mutate_after_download.set(1);
    assert!(
        crate::crate_release_tool_artifact::fixture_acquire_receipt(
            stage,
            github,
            registry,
            github,
            crate::CrateSource::new(source, test_git(), &crate::UnprivilegedSourceExecution)
        )
        .is_err()
    );
    assert!(
        !stage
            .runner_temp
            .join("velnor-crate-release-receipt.json")
            .exists()
    );
    Ok(())
}

pub(super) fn check_receipt_revalidation(
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
    assert!(
        fixture_acquire_receipt(
            stage,
            github,
            registry,
            github,
            crate::CrateSource::new(source, test_git(), &crate::UnprivilegedSourceExecution)
        )
        .is_err()
    );
    assert!(
        !stage
            .runner_temp
            .join("velnor-crate-release-receipt.json")
            .exists()
    );
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(receipt)?)?;
    *registry.archive.borrow_mut() = fixture_archive(source)?;
    registry.absent.set(false);
    fixture_acquire_receipt(
        stage,
        github,
        registry,
        github,
        crate::CrateSource::new(source, test_git(), &crate::UnprivilegedSourceExecution),
    )?;
    verify_crate_release_admission(
        receipt,
        stage.intent,
        stage.github,
        github,
        registry,
        crate::CrateSource::new(source, test_git(), &crate::UnprivilegedSourceExecution),
    )?;
    fs::remove_file(stage.runner_temp.join("velnor-crate-release-receipt.json"))?;
    Ok(())
}

pub(super) fn receipt_zip(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use std::io::Write as _;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file(
        "velnor-crate-release-receipt.json",
        zip::write::SimpleFileOptions::default(),
    )?;
    zip.write_all(bytes)?;
    Ok(zip.finish()?.into_inner())
}

pub(super) fn check_authentication_denials(
    stage: &crate::crate_release_tool_artifact::ToolchainStageRequest<'_>,
    github: &Github,
    receipt: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_tool_artifact::ToolchainStageRequest;
    let mut wrong_intent_receipt = receipt.clone();
    wrong_intent_receipt.intent.manifest_path = "other/Cargo.toml".to_owned();
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&wrong_intent_receipt)?)?;
    assert!(
        crate::crate_release_tool_artifact::fixture_authenticate_receipt(stage, github, github)
            .is_err()
    );
    let mut wrong_link_intent = stage.intent.clone();
    wrong_link_intent.environment = "different".to_owned();
    let mut wrong_link_receipt = receipt.clone();
    wrong_link_receipt.intent = wrong_link_intent.clone();
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&wrong_link_receipt)?)?;
    let wrong_link_stage = ToolchainStageRequest {
        intent: &wrong_link_intent,
        ..*stage
    };
    assert!(
        crate::crate_release_tool_artifact::fixture_authenticate_receipt(
            &wrong_link_stage,
            github,
            github
        )
        .is_err()
    );
    let mut wrong_schema_receipt = receipt.clone();
    wrong_schema_receipt.schema = 2;
    *github.tool_archive.borrow_mut() = receipt_zip(&serde_json::to_vec(&wrong_schema_receipt)?)?;
    assert!(
        crate::crate_release_tool_artifact::fixture_authenticate_receipt(stage, github, github)
            .is_err()
    );
    Ok(())
}

pub(super) fn check_transient_repository(
    stage: &crate::crate_release_tool_artifact::ToolchainStageRequest<'_>,
    github: &Github,
) {
    struct TransientRepository<'a> {
        github: &'a Github,
        phase: Cell<u8>,
        downloads: Cell<u32>,
    }
    impl GitHubAdmissionRead for TransientRepository<'_> {
        fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
            let mut value = self.github.get(route)?;
            if route.ends_with("/actions/runs/12/approvals") && self.phase.get() == 0 {
                self.phase.set(1);
            } else if route == "repos/example/demo" {
                match self.phase.get() {
                    1 => self.phase.set(2),
                    2 => {
                        self.phase.set(3);
                        value["id"] = json!(23);
                    }
                    _ => {}
                }
            } else if self.phase.get() == 3 && route.ends_with("/actions/runs/12") {
                value["repository"]["id"] = json!(23);
                value["head_repository"]["id"] = json!(23);
            } else if self.phase.get() == 3
                && route.ends_with("/actions/runs/12/artifacts?per_page=100")
            {
                value["artifacts"][0]["workflow_run"]["repository_id"] = json!(23);
                value["artifacts"][0]["workflow_run"]["head_repository_id"] = json!(23);
            } else if self.phase.get() == 3 && route.ends_with("/actions/artifacts/10") {
                value["workflow_run"]["repository_id"] = json!(23);
                value["workflow_run"]["head_repository_id"] = json!(23);
                self.phase.set(4);
            }
            Ok(value)
        }
        fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
            self.github.get_optional(route)
        }
    }
    impl crate::crate_release_tool_download::ToolDownloadTransport for TransientRepository<'_> {
        fn github(
            &self,
            repo: &str,
            id: u64,
            token: &str,
            deadline: std::time::Instant,
        ) -> Result<crate::crate_release_tool_download::ToolDownloadResponse, OrchestratorError>
        {
            self.downloads.set(self.downloads.get() + 1);
            self.github.github(repo, id, token, deadline)
        }
        fn storage(
            &self,
            url: &str,
            deadline: std::time::Instant,
        ) -> Result<crate::crate_release_tool_download::ToolDownloadResponse, OrchestratorError>
        {
            self.github.storage(url, deadline)
        }
    }
    let transient = TransientRepository {
        github,
        phase: Cell::new(0),
        downloads: Cell::new(0),
    };
    assert!(
        crate::crate_release_tool_artifact::fixture_authenticate_receipt(
            stage, &transient, &transient
        )
        .is_err()
    );
    assert_eq!(
        transient.phase.get(),
        4,
        "artifact identity was substituted"
    );
    assert_eq!(
        transient.downloads.get(),
        0,
        "unauthorized artifact must not be requested"
    );
}
