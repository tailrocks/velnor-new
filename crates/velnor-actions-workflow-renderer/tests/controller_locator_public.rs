// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, error::Error};

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::render::{
    CanonicalAuthoritySteps, CanonicalRuntimeInputs, ControllerLocator,
    PROTECTED_BASE_CONTROLLER_PATH,
};

use super::{context, plan_env, repository_config};

fn pinned(version: &str) -> ControllerLocator {
    ControllerLocator::PinnedRelease {
        version: version.to_owned(),
        path: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{version}"),
    }
}

#[test]
fn controller_locators_preserve_release_identity_and_fix_dispatch_location()
-> Result<(), Box<dyn Error>> {
    let release = pinned("0.1.74");
    release.validate()?;
    assert_eq!(release.require_pinned_version()?, "0.1.74");
    let dispatch = ControllerLocator::ProtectedBaseDispatch {
        version: "0.1.75".to_owned(),
        path: PROTECTED_BASE_CONTROLLER_PATH.to_owned(),
    };
    dispatch.validate()?;
    assert_eq!(dispatch.pinned_version(), None);
    assert_eq!(dispatch.report_helper_version(), "0.1.75");
    assert!(dispatch.require_pinned_version().is_err());
    assert!(
        ControllerLocator::ProtectedBaseDispatch {
            version: "0.1.75-dev".to_owned(),
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
                version: "0.1.75".to_owned(),
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
            version: "0.1.75".to_owned(),
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
        version: "0.1.75".to_owned(),
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
            version: "0.1.75".to_owned(),
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
