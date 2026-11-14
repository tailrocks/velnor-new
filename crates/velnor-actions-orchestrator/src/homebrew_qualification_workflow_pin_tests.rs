// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{A_ADMISSION, load_catalog};
use crate::OrchestratorError;
use crate::package_release_public::{
    HistoricalPackageReleaseAdmission, HistoricalWorkflowPin, ObservedWorkflow,
};

const PREVIEW_PATH: &str = ".github/workflows/binary-preview.yml";
const PREVIEW_NAME: &str = "Rust binary preview";
const PREVIEW_ID: u64 = 388_393_922;

fn pinned_a_and_d() -> Result<[HistoricalPackageReleaseAdmission; 2], OrchestratorError> {
    let (plan, _spec, a, d) = load_catalog()?;
    Ok([
        super::admission(&a, &plan, Some(A_ADMISSION))?,
        super::admission(&d, &plan, None)?,
    ])
}

fn pin_of(
    admission: &HistoricalPackageReleaseAdmission,
) -> Result<&HistoricalWorkflowPin, OrchestratorError> {
    admission
        .workflow_pin()
        .ok_or_else(|| crate::internal::internal("missing_workflow_pin"))
}

fn observed(
    id: u64,
    path: &'static str,
    name: &'static str,
    state: &'static str,
) -> ObservedWorkflow<'static> {
    ObservedWorkflow {
        id,
        path,
        name,
        state,
    }
}

#[test]
fn historical_a_and_d_admit_the_deleted_producer_workflow() -> Result<(), OrchestratorError> {
    for admission in &pinned_a_and_d()? {
        assert!(pin_of(admission)?.admits(
            &observed(PREVIEW_ID, PREVIEW_PATH, PREVIEW_NAME, "deleted"),
            PREVIEW_PATH,
            PREVIEW_NAME
        ));
    }
    Ok(())
}

#[test]
fn historical_a_and_d_still_admit_an_active_producer_workflow() -> Result<(), OrchestratorError> {
    for admission in &pinned_a_and_d()? {
        assert!(pin_of(admission)?.admits(
            &observed(PREVIEW_ID, PREVIEW_PATH, PREVIEW_NAME, "active"),
            PREVIEW_PATH,
            PREVIEW_NAME
        ));
    }
    Ok(())
}

#[test]
fn a_deleted_workflow_with_any_other_identity_is_rejected_for_historical_a_and_d()
-> Result<(), OrchestratorError> {
    for admission in &pinned_a_and_d()? {
        let pin = pin_of(admission)?;
        let other_id = observed(PREVIEW_ID + 1, PREVIEW_PATH, PREVIEW_NAME, "deleted");
        let other_name = observed(PREVIEW_ID, PREVIEW_PATH, "Rust binary release", "deleted");
        let other_path = observed(
            PREVIEW_ID,
            ".github/workflows/binary-release.yml",
            PREVIEW_NAME,
            "deleted",
        );
        let disabled = observed(PREVIEW_ID, PREVIEW_PATH, PREVIEW_NAME, "disabled_manually");
        assert!(!pin.admits(&other_id, PREVIEW_PATH, PREVIEW_NAME));
        assert!(!pin.admits(&other_name, PREVIEW_PATH, PREVIEW_NAME));
        assert!(!pin.admits(&other_path, PREVIEW_PATH, PREVIEW_NAME));
        assert!(!pin.admits(&disabled, PREVIEW_PATH, PREVIEW_NAME));
        assert!(!pin.admits(
            &observed(PREVIEW_ID, PREVIEW_PATH, PREVIEW_NAME, "deleted"),
            PREVIEW_PATH,
            "Rust binary release"
        ));
    }
    Ok(())
}

#[test]
fn an_admission_without_a_pin_has_no_deleted_workflow_exception() -> Result<(), OrchestratorError>
{
    let (plan, _spec, a, _d) = load_catalog()?;
    let unpinned = HistoricalPackageReleaseAdmission::new_from_protected_policy(
        &plan,
        a.release.source_sha.clone(),
        a.release.tag.clone(),
        a.release.id,
        a.release.receipt_asset_id,
        a.release.receipt_sha256.clone(),
        a.plan_sha256.clone(),
    )?;
    assert!(unpinned.workflow_pin().is_none());
    Ok(())
}
