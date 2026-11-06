// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error};

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::render::{
    CanonicalAuthoritySteps, CanonicalRuntimeInputs, ControllerLocator,
    PROTECTED_BASE_CONTROLLER_PATH,
};

use super::{context, plan_env, repository_config, workflow};

fn pinned(version: &str) -> ControllerLocator {
    ControllerLocator::PinnedRelease {
        version: version.to_owned(),
        path: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{version}"),
    }
}

fn preview() -> velnor_actions_contract::source_preview::SourcePreviewLock {
    use velnor_actions_contract::source_preview::{
        SourcePreviewCandidate, SourcePreviewLock, SourcePreviewPayload, SourcePreviewProducer,
        SourcePreviewReceipt, SourcePreviewRun,
    };
    let source = "a".repeat(40);
    let run = |path: &str, reference: &str, id| SourcePreviewRun {
        workflow_path: path.to_owned(),
        workflow_ref: format!("tailrocks/velnor-new/{path}@{reference}"),
        workflow_sha: source.clone(),
        run_id: id,
        run_attempt: 1,
    };
    SourcePreviewLock {
        receipt: SourcePreviewReceipt {
            schema: 2,
            repository: "tailrocks/velnor-new".to_owned(),
            version: "0.1.75".to_owned(),
            source_sha: source.clone(),
            tag: format!("sdk-preview-{source}"),
            release_id: 1,
            source: run(
                ".github/workflows/product-release-generator.yml",
                &format!("refs/tags/sdk-preview-{source}"),
                1,
            ),
            draft: run(
                ".github/workflows/product-release-generator.yml",
                &format!("refs/tags/sdk-preview-{source}"),
                1,
            ),
            signer: run(
                ".github/workflows/product-release-generator.yml",
                &format!("refs/tags/sdk-preview-{source}"),
                1,
            ),
            ci: run(".github/workflows/ci.yml", "refs/heads/main", 2),
            qualification: run(
                ".github/workflows/source-qualification.yml",
                "refs/heads/main",
                3,
            ),
            producer: SourcePreviewProducer {
                producer_run_id: 4,
                producer_run_attempt: 1,
                payload_artifact_id: 5,
                payload_archive_sha256: "e".repeat(64),
            },
            candidate: SourcePreviewCandidate {
                source_artifact_id: 6,
                payload_artifact_id: 7,
            },
            payload: SourcePreviewPayload {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                name: "velnor-actions-x86_64-unknown-linux-gnu".to_owned(),
                size: 1,
                sha256: "b".repeat(64),
            },
        },
        receipt_sha256: "c".repeat(64),
        bundle_sha256: "d".repeat(64),
        publisher_run_attempt: 1,
        publisher_job_id: 8,
    }
}

#[test]
fn controller_locators_preserve_release_identity_and_fix_dispatch_location()
-> Result<(), Box<dyn Error>> {
    let release = pinned("0.1.74");
    release.validate()?;
    assert_eq!(release.require_pinned_version()?, "0.1.74");
    let dispatch = ControllerLocator::ProtectedBaseDispatch {
        preview: Box::new(preview()),
        path: PROTECTED_BASE_CONTROLLER_PATH.to_owned(),
    };
    dispatch.validate()?;
    assert_eq!(dispatch.pinned_version(), None);
    assert_eq!(dispatch.report_helper_version(), "0.1.75");
    assert!(dispatch.require_pinned_version().is_err());
    assert!(
        ControllerLocator::ProtectedBaseDispatch {
            preview: {
                let mut value = preview();
                value.receipt.version = "0.1.75-dev".to_owned();
                Box::new(value)
            },
            path: PROTECTED_BASE_CONTROLLER_PATH.to_owned(),
        }
        .validate()
        .is_err()
    );
    for path in [
        release.path(),
        "$OTHER_TEMP/velnor/bin/velnor-actions-controller",
        "$(id)",
    ] {
        assert!(
            ControllerLocator::ProtectedBaseDispatch {
                preview: Box::new(preview()),
                path: path.to_owned(),
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        ControllerLocator::PinnedRelease {
            version: "0.1.75".to_owned(),
            path: release.path().to_owned(),
        }
        .validate()
        .is_err()
    );
    for version in ["01.1.74", "0.1.74;id", "0.1.74-dev", "0.1.74+build"] {
        assert!(pinned(version).validate().is_err());
    }
    Ok(())
}

#[test]
fn runtime_constructor_rejects_invalid_locator_before_qualification() {
    let result = CanonicalRuntimeInputs::new(
        plan_env(),
        ControllerLocator::ProtectedBaseDispatch {
            preview: Box::new(preview()),
            path: pinned("0.1.74").path().to_owned(),
        },
        "$RUNNER_TEMP/velnor/requests/request".to_owned(),
        "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683".to_owned(),
        false,
    );
    assert!(result.is_err());
}

#[test]
fn renderer_rejects_a_valid_locator_different_from_qualified_authority()
-> Result<(), Box<dyn Error>> {
    let mut context = context(&BTreeMap::new())?;
    context.validate()?;
    context.controller = pinned("0.1.74");
    assert!(context.validate().is_err());
    context.controller = ControllerLocator::ProtectedBaseDispatch {
        preview: Box::new(preview()),
        path: PROTECTED_BASE_CONTROLLER_PATH.to_owned(),
    };
    assert!(context.validate().is_err());
    Ok(())
}

#[test]
fn consumer_authority_rejects_unapproved_dispatch_controller() -> Result<(), Box<dyn Error>> {
    let runtime = CanonicalRuntimeInputs::new(
        plan_env(),
        ControllerLocator::ProtectedBaseDispatch {
            preview: Box::new(preview()),
            path: PROTECTED_BASE_CONTROLLER_PATH.to_owned(),
        },
        "$RUNNER_TEMP/velnor/requests/request".to_owned(),
        "actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683".to_owned(),
        false,
    )?;
    assert!(
        CanonicalAuthoritySteps::qualify_jobs(
            &BTreeMap::new(),
            WorkflowPolicy::ConsumerV1,
            &repository_config()?,
            "ubuntu-26.04",
            Vec::new(),
            runtime,
        )
        .is_err()
    );
    context(&BTreeMap::new())?.validate()?;
    Ok(())
}

#[test]
fn preview_setup_action_requires_its_typed_role() -> Result<(), Box<dyn Error>> {
    let step = velnor_actions_workflow_renderer::steps::protected_preview_acquire_step()?;
    let approved = workflow(step.clone());
    approved.validate()?;
    let mut untyped = step;
    untyped.role = None;
    assert!(
        workflow(untyped).validate().is_err(),
        "untyped preview setup action was accepted"
    );
    Ok(())
}
