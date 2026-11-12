// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{A_ADMISSION, REPOSITORY_ID, load_catalog};
use crate::package_release_public::{ObservedWorkflow, PackageReleasePublicPolicy};

const PREVIEW_PATH: &str = ".github/workflows/binary-preview.yml";
const PREVIEW_NAME: &str = "Rust binary preview";
const PREVIEW_ID: u64 = 388_393_922;

fn policy_and_tags() -> Result<(PackageReleasePublicPolicy, String, String), crate::OrchestratorError>
{
    let (plan, _spec, a, d) = load_catalog()?;
    let policy = PackageReleasePublicPolicy::from_trusted_source_authority(
        plan.clone(),
        REPOSITORY_ID,
        "0".repeat(40),
    )?
    .with_historical_admissions(vec![
        super::admission(&a, &plan, Some(A_ADMISSION))?,
        super::admission(&d, &plan, None)?,
    ])?;
    Ok((policy, a.release.tag.clone(), d.release.tag.clone()))
}

fn observed(id: u64, path: &'static str, name: &'static str, state: &'static str) -> ObservedWorkflow<'static> {
    ObservedWorkflow {
        id,
        path,
        name,
        state,
    }
}

#[test]
fn pinned_historical_releases_admit_the_deleted_producer_workflow() -> Result<(), crate::OrchestratorError> {
    let (policy, a_tag, d_tag) = policy_and_tags()?;
    for tag in [&a_tag, &d_tag] {
        let pin = policy
            .historical_workflow_pin(tag)
            .ok_or_else(|| crate::internal::internal("missing_pin"))?;
        assert!(pin.admits(
            &observed(PREVIEW_ID, PREVIEW_PATH, PREVIEW_NAME, "deleted"),
            PREVIEW_PATH,
            PREVIEW_NAME
        ));
    }
    Ok(())
}

#[test]
fn pinned_historical_releases_still_admit_an_active_producer_workflow() -> Result<(), crate::OrchestratorError> {
    let (policy, a_tag, d_tag) = policy_and_tags()?;
    for tag in [&a_tag, &d_tag] {
        let pin = policy
            .historical_workflow_pin(tag)
            .ok_or_else(|| crate::internal::internal("missing_pin"))?;
        assert!(pin.admits(
            &observed(PREVIEW_ID, PREVIEW_PATH, PREVIEW_NAME, "active"),
            PREVIEW_PATH,
            PREVIEW_NAME
        ));
    }
    Ok(())
}

#[test]
fn a_deleted_workflow_with_any_other_identity_is_rejected_for_pinned_releases() -> Result<(), crate::OrchestratorError> {
    let (policy, a_tag, _d_tag) = policy_and_tags()?;
    let pin = policy
        .historical_workflow_pin(&a_tag)
        .ok_or_else(|| crate::internal::internal("missing_pin"))?;
    let other_id = observed(PREVIEW_ID + 1, PREVIEW_PATH, PREVIEW_NAME, "deleted");
    let other_name = observed(PREVIEW_ID, PREVIEW_PATH, "Rust binary release", "deleted");
    let other_path = observed(PREVIEW_ID, ".github/workflows/binary-release.yml", PREVIEW_NAME, "deleted");
    let zero_id = observed(0, PREVIEW_PATH, PREVIEW_NAME, "deleted");
    let disabled = observed(PREVIEW_ID, PREVIEW_PATH, PREVIEW_NAME, "disabled_manually");
    assert!(!pin.admits(&other_id, PREVIEW_PATH, PREVIEW_NAME));
    assert!(!pin.admits(&other_name, PREVIEW_PATH, PREVIEW_NAME));
    assert!(!pin.admits(&other_path, PREVIEW_PATH, PREVIEW_NAME));
    assert!(!pin.admits(&zero_id, PREVIEW_PATH, PREVIEW_NAME));
    assert!(!pin.admits(&disabled, PREVIEW_PATH, PREVIEW_NAME));
    assert!(!pin.admits(
        &observed(PREVIEW_ID, PREVIEW_PATH, PREVIEW_NAME, "deleted"),
        PREVIEW_PATH,
        "Rust binary release"
    ));
    Ok(())
}

#[test]
fn an_unpinned_release_has_no_deleted_workflow_exception() -> Result<(), crate::OrchestratorError> {
    let (policy, _a_tag, _d_tag) = policy_and_tags()?;
    let unpinned = format!("preview-{}", "1".repeat(40));
    assert!(policy.historical_workflow_pin(&unpinned).is_none());
    Ok(())
}
