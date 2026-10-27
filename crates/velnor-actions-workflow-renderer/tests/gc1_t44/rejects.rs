// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_workflow_renderer::preseed::{
    PRESEED_MANIFEST_INPUT_DIGEST_ENV, PreseedStageSource, preseed_build_step,
    preseed_manifest_step, preseed_stage_step, source_bound_helper_digest,
};

use super::{
    build_argv, consumer_handoff, expect_order_error, fail, plan_handoff, shell_role_step, staged,
    target,
};

pub(super) fn check_rejects_unkeyed_helper() -> Result<(), Box<dyn Error>> {
    if source_bound_helper_digest(&build_argv(), "not-a-target").is_ok() {
        return Err(fail("helper digest accepted its bad target"));
    }
    if source_bound_helper_digest(&["mbx".to_owned(), "build".to_owned()], target()).is_ok() {
        return Err(fail("helper digest accepted its bad build"));
    }
    if preseed_manifest_step(&build_argv(), "not-a-target").is_ok() {
        return Err(fail("manifest accepted its bad target"));
    }
    let mut unkeyed = plan_handoff()?;
    let manifest_at = unkeyed
        .iter()
        .position(|step| step.role == Some(StepRole::PreseedManifest))
        .ok_or_else(|| fail("handoff left its manifest"))?;
    match &mut unkeyed[manifest_at].kind {
        StepKind::Shell { env, .. } => {
            env.remove(PRESEED_MANIFEST_INPUT_DIGEST_ENV);
        }
        _ => return Err(fail("manifest left its shell shape")),
    }
    expect_order_error(
        "velnor-plan",
        &unkeyed,
        true,
        "preseed_manifest_without_input_digest",
    )?;
    let mut malformed = plan_handoff()?;
    let manifest_at = malformed
        .iter()
        .position(|step| step.role == Some(StepRole::PreseedManifest))
        .ok_or_else(|| fail("handoff left its manifest"))?;
    match &mut malformed[manifest_at].kind {
        StepKind::Shell { env, .. } => {
            env.insert(
                PRESEED_MANIFEST_INPUT_DIGEST_ENV.to_owned(),
                "not-a-digest".to_owned(),
            );
        }
        _ => return Err(fail("manifest left its shell shape")),
    }
    expect_order_error(
        "velnor-plan",
        &malformed,
        true,
        "preseed_manifest_without_input_digest",
    )?;
    Ok(())
}

pub(super) fn check_rejects_unverified_build_order() -> Result<(), Box<dyn Error>> {
    let mut late_restore = plan_handoff()?;
    late_restore.remove(0);
    expect_order_error(
        "velnor-plan",
        &late_restore,
        true,
        "preseed_build_without_restore",
    )?;
    let mut late_controller = plan_handoff()?;
    late_controller.insert(
        1,
        shell_role_step("Acquire Velnor", StepRole::AcquireVelnor),
    );
    let acquire = late_controller.remove(1);
    late_controller.push(acquire);
    expect_order_error(
        "velnor-plan",
        &late_controller,
        true,
        "preseed_controller_after_build",
    )?;
    Ok(())
}

pub(super) fn check_rejects_cache_cycle() -> Result<(), Box<dyn Error>> {
    let mut save_inside = plan_handoff()?;
    save_inside.insert(2, shell_role_step("Save tools", StepRole::ToolsCacheSave));
    expect_order_error("velnor-plan", &save_inside, true, "preseed_cache_cycle")?;
    let mut restore_inside = plan_handoff()?;
    restore_inside.insert(
        3,
        shell_role_step("Restore sources", StepRole::CargoSourcesRestore),
    );
    expect_order_error("velnor-plan", &restore_inside, true, "preseed_cache_cycle")?;
    let mut consumer_cycle = consumer_handoff()?;
    consumer_cycle.insert(1, shell_role_step("Save tools", StepRole::ToolsCacheSave));
    expect_order_error(
        "crate-velnor-actions-cli",
        &consumer_cycle,
        false,
        "preseed_cache_cycle",
    )?;
    Ok(())
}

pub(super) fn check_rejects_context_crossing() -> Result<(), Box<dyn Error>> {
    let mut consumer_builds = consumer_handoff()?;
    consumer_builds.insert(1, preseed_build_step(&build_argv(), &BTreeMap::new())?);
    expect_order_error(
        "crate-velnor-actions-cli",
        &consumer_builds,
        false,
        "preseed_rebuild_in_consumer",
    )?;
    let mut consumer_compiles = consumer_handoff()?;
    consumer_compiles.push(preseed_stage_step(
        PreseedStageSource::LocalBuild,
        &staged(),
    )?);
    expect_order_error(
        "crate-velnor-actions-cli",
        &consumer_compiles,
        false,
        "preseed_stage_context",
    )?;
    let mut plan_downloads = plan_handoff()?;
    let stage_at = plan_downloads
        .iter()
        .position(|step| step.role == Some(StepRole::PreseedStage))
        .ok_or_else(|| fail("handoff left its stage"))?;
    plan_downloads[stage_at] =
        preseed_stage_step(PreseedStageSource::DownloadedArtifact, &staged())?;
    expect_order_error(
        "velnor-plan",
        &plan_downloads,
        true,
        "preseed_stage_context",
    )?;
    Ok(())
}
