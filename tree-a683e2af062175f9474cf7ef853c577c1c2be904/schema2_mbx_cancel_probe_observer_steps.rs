//! Fresh observer step construction and evidence gates.

use std::collections::BTreeMap;

use super::scripts;
use super::util::{controller_receipt_env, phase_env};
use super::{MbxQualificationPins, Phase};
use crate::RenderError;
use crate::yaml::Yaml;

pub(super) fn observer_cache_before_step(
    request: &MbxQualificationPins,
    phase: Phase,
) -> Result<Yaml, RenderError> {
    let mut env = phase_env(request, phase);
    env.extend(controller_receipt_env());
    env.extend([
        (
            "RUN_ID".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.child_run_id }}".to_owned(),
        ),
        (
            "WORKFLOW_ID".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.child_workflow_id }}".to_owned(),
        ),
    ]);
    super::super::render::token_bash_step(
        "Record exact cache state before observer restore",
        None,
        scripts::OBSERVER_CACHE_BEFORE,
        &env,
        Some("steps.mbx-cancel-receipt.outputs.should_observe == 'true'"),
    )
}

pub(super) fn observer_evidence_step(
    request: &MbxQualificationPins,
    phase: Phase,
) -> Result<Yaml, RenderError> {
    let mut env = phase_env(request, phase);
    env.extend(controller_receipt_env());
    env.extend([
        (
            "RUN_ID".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.child_run_id }}".to_owned(),
        ),
        (
            "WORKFLOW_ID".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.child_workflow_id }}".to_owned(),
        ),
    ]);
    super::super::render::token_bash_step(
        "Record exact child cache and upload evidence",
        None,
        scripts::OBSERVER_EVIDENCE,
        &env,
        Some("always() && steps.mbx-cancel-receipt.outputs.should_observe == 'true'"),
    )
}

pub(super) fn observer_measure_import_step() -> Yaml {
    super::super::render::bash_step_if(
        "Measure imported MBX objects",
        scripts::OBSERVER_IMPORT_MEASURE,
        &BTreeMap::new(),
        Some("steps.mbx-cancel-receipt.outputs.should_observe == 'true'"),
    )
}

pub(super) fn observer_measure_reuse_step() -> Yaml {
    super::super::render::bash_step_if(
        "Measure cached compiler reuse",
        scripts::OBSERVER_REUSE_MEASURE,
        &BTreeMap::new(),
        Some("steps.mbx-cancel-receipt.outputs.should_observe == 'true'"),
    )
}

pub(super) fn observer_classify_step(request: &MbxQualificationPins, phase: Phase) -> Yaml {
    let mut env = phase_env(request, phase);
    env.extend(controller_receipt_env());
    env.extend([
        (
            "SHOULD_OBSERVE".to_owned(),
            "${{ steps.mbx-cancel-receipt.outputs.should_observe }}".to_owned(),
        ),
        (
            "DERIVED_KEY".to_owned(),
            "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
        ),
        (
            "RESTORE_HIT".to_owned(),
            "${{ steps.mbx-bundle.outputs.cache-hit }}".to_owned(),
        ),
        (
            "MATCHED_KEY".to_owned(),
            "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
        ),
        (
            "GENERATION".to_owned(),
            velnor_actions_contract::cachekey::mbx_cache_generation(&request.mbx_version),
        ),
        ("MBX_VERSION".to_owned(), request.mbx_version.clone()),
        ("CACHE_SCOPE".to_owned(), phase.scope().to_owned()),
    ]);
    super::super::render::bash_step_if(
        "Record observer outcome",
        scripts::OBSERVER_CLASSIFY,
        &env,
        Some("always()"),
    )
}
