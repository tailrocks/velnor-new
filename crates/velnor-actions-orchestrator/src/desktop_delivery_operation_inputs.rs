//! Stage admission for typed Swift consumer inputs.

use super::{NativeSwiftStage, invalid};
use velnor_actions_contract::config::SwiftInputs;
use velnor_actions_workflow_renderer::RenderError;

pub(super) fn validate_stage_inputs(
    stage: &NativeSwiftStage,
    inputs: &SwiftInputs,
) -> Result<(), RenderError> {
    validate_stage_values(stage)?;
    let requires_apple = matches!(
        stage,
        NativeSwiftStage::GenerateProject
            | NativeSwiftStage::Build(..)
            | NativeSwiftStage::Verify(..)
            | NativeSwiftStage::XcodeBuild
            | NativeSwiftStage::XcodeTest
            | NativeSwiftStage::UiTest
            | NativeSwiftStage::Deadcode
            | NativeSwiftStage::Sign(..)
            | NativeSwiftStage::State(..)
    );
    if requires_apple && inputs.apple.is_none() {
        return Err(invalid("native_swift_stage_requires_apple"));
    }
    let checks = inputs.checks.as_ref();
    if matches!(
        stage,
        NativeSwiftStage::SwiftTest
            | NativeSwiftStage::Format
            | NativeSwiftStage::Lint
            | NativeSwiftStage::SwiftHarnesses
    ) && checks.is_none()
    {
        return Err(invalid("native_swift_stage_requires_checks"));
    }
    if matches!(stage, NativeSwiftStage::SwiftTest)
        && checks.is_none_or(|value| value.swift_test_frameworks.is_empty())
    {
        return Err(invalid("native_swift_stage_requires_swift_tests"));
    }
    if matches!(stage, NativeSwiftStage::SwiftHarnesses)
        && checks.is_none_or(|value| value.swift_harness_products.is_empty())
    {
        return Err(invalid("native_swift_stage_requires_harness_products"));
    }
    if matches!(stage, NativeSwiftStage::XcodeTest)
        && inputs
            .apple
            .as_ref()
            .is_none_or(|value| value.test_target.is_none())
    {
        return Err(invalid("native_swift_stage_requires_test_target"));
    }
    if matches!(stage, NativeSwiftStage::UiTest)
        && inputs
            .apple
            .as_ref()
            .is_none_or(|value| value.ui_test_target.is_none())
    {
        return Err(invalid("native_swift_stage_requires_ui_test_target"));
    }
    Ok(())
}

fn validate_stage_values(stage: &NativeSwiftStage) -> Result<(), RenderError> {
    match stage {
        NativeSwiftStage::Build(version, build)
        | NativeSwiftStage::Sign(version, build)
        | NativeSwiftStage::Verify(version, build, _, _) => {
            if !version_parts(version) {
                return Err(invalid("native_swift_stage_version_literal"));
            }
            if !build.chars().all(|value| value.is_ascii_digit()) || build.is_empty() {
                return Err(invalid("native_swift_stage_build_literal"));
            }
            if let NativeSwiftStage::Verify(_, _, _, Some(path)) = stage {
                super::validate_relative_path(path)?;
            }
        }
        NativeSwiftStage::State(version, repository) => {
            if !version_parts(version) {
                return Err(invalid("native_swift_stage_version_literal"));
            }
            if !repository_parts(repository) {
                return Err(invalid("native_swift_stage_repository_literal"));
            }
        }
        _ => {}
    }
    Ok(())
}

fn version_parts(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|value| value.is_ascii_digit()))
}

fn repository_parts(value: &str) -> bool {
    let mut parts = value.split('/');
    parts.next().is_some_and(component)
        && parts.next().is_some_and(component)
        && parts.next().is_none()
}

fn component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'))
}
