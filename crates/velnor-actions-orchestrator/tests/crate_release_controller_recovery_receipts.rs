// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
