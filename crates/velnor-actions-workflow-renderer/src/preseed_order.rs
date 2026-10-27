// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;

use super::{PRESEED_BUILD_OUTPUT, PRESEED_DOWNLOADED_BINARY, PRESEED_MANIFEST_INPUT_DIGEST_ENV};

fn manifest_input_digest(step: &Step) -> Option<&str> {
    match &step.kind {
        velnor_actions_contract::StepKind::Shell { env, .. } => env
            .get(PRESEED_MANIFEST_INPUT_DIGEST_ENV)
            .map(String::as_str),
        _ => None,
    }
}

fn step_mentions(step: &Step, needle: &str) -> bool {
    match &step.kind {
        velnor_actions_contract::StepKind::Shell { run, .. } => {
            run.iter().any(|arg| arg.contains(needle))
        }
        _ => false,
    }
}

fn is_cache_save(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::ToolsCacheSave
            | StepRole::CargoSourcesSave
            | StepRole::MbxBundleSave
            | StepRole::FamilySave
            | StepRole::TofuProvidersSave
    )
}

fn is_cache_restore(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::ToolsCacheRestore
            | StepRole::CargoSourcesRestore
            | StepRole::MbxBundleRestore
            | StepRole::FamilyRestore
            | StepRole::TofuProvidersRestore
    )
}

fn is_tool_state_restore(role: StepRole) -> bool {
    matches!(role, StepRole::PreparePinnedTools | StepRole::MbxCache) || is_cache_restore(role)
}

fn check_plan_build_order(
    id: &str,
    steps: &[Step],
    build_at: usize,
    position: &impl Fn(StepRole) -> Option<usize>,
) -> Result<(), RenderError> {
    if let Some(acquire_at) = position(StepRole::AcquireVelnor)
        && acquire_at > build_at
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "preseed_controller_after_build:{id}"
        )));
    }
    if !steps
        .iter()
        .take(build_at)
        .any(|step| step.role.is_some_and(is_tool_state_restore))
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "preseed_build_without_restore:{id}"
        )));
    }
    let upload_at = position(StepRole::PreseedUpload).unwrap_or(build_at);
    if steps
        .iter()
        .skip(build_at)
        .take(upload_at.saturating_sub(build_at) + 1)
        .any(|step| {
            step.role
                .is_some_and(|role| is_cache_save(role) || is_cache_restore(role))
        })
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "preseed_cache_cycle:{id}"
        )));
    }
    Ok(())
}

fn check_plan_stage_source(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps
        .iter()
        .filter(|step| step.role == Some(StepRole::PreseedStage))
    {
        if !step_mentions(step, PRESEED_BUILD_OUTPUT)
            || step_mentions(step, PRESEED_DOWNLOADED_BINARY)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_stage_context:{id}"
            )));
        }
    }
    Ok(())
}

fn check_consumer_reuse_order(
    id: &str,
    steps: &[Step],
    position: &impl Fn(StepRole) -> Option<usize>,
) -> Result<(), RenderError> {
    if steps.iter().any(|step| {
        matches!(
            step.role,
            Some(
                StepRole::PreseedBuild
                    | StepRole::PreseedVerifyBuild
                    | StepRole::PreseedManifest
                    | StepRole::PreseedUpload
            )
        )
    }) {
        return Err(RenderError::InvalidWorkflow(format!(
            "preseed_rebuild_in_consumer:{id}"
        )));
    }
    let (Some(download_at), Some(stage_at)) = (
        position(StepRole::PreseedDownload),
        position(StepRole::PreseedStage),
    ) else {
        return Ok(());
    };
    if steps
        .iter()
        .skip(download_at)
        .take(stage_at.saturating_sub(download_at) + 1)
        .any(|step| {
            step.role
                .is_some_and(|role| is_cache_save(role) || is_cache_restore(role))
        })
    {
        return Err(RenderError::InvalidWorkflow(format!(
            "preseed_cache_cycle:{id}"
        )));
    }
    for step in steps
        .iter()
        .skip(download_at)
        .filter(|step| step.role == Some(StepRole::PreseedStage))
    {
        if !step_mentions(step, PRESEED_DOWNLOADED_BINARY)
            || step_mentions(step, PRESEED_BUILD_OUTPUT)
        {
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_stage_context:{id}"
            )));
        }
    }
    Ok(())
}

fn check_manifest_digests(id: &str, steps: &[Step]) -> Result<(), RenderError> {
    for step in steps
        .iter()
        .filter(|step| step.role == Some(StepRole::PreseedManifest))
    {
        let digest = manifest_input_digest(step);
        if digest.is_none_or(|value| !velnor_actions_contract::ids::is_lower_hex_len(value, 64)) {
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_manifest_without_input_digest:{id}"
            )));
        }
    }
    Ok(())
}

pub fn check_preseed_step_order(
    id: &str,
    steps: &[Step],
    is_plan: bool,
) -> Result<(), RenderError> {
    let position = |role| steps.iter().position(|step| step.role == Some(role));
    if is_plan {
        if let Some(build_at) = position(StepRole::PreseedBuild) {
            check_plan_build_order(id, steps, build_at, &position)?;
        }
        check_plan_stage_source(id, steps)?;
    } else {
        check_consumer_reuse_order(id, steps, &position)?;
    }
    check_manifest_digests(id, steps)
}
