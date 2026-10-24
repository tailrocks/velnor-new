use super::{Step, StepKind};
use crate::errors::ContractError;

fn expected_qualification_cache_composite_call(
    phase: crate::workflow::step_id::QualificationCacheCompositePhase,
    layer: Option<crate::workflow::qualification_cache_lineage::QualificationCacheLayer>,
    job: &str,
) -> Result<(String, std::collections::BTreeMap<String, String>), ContractError> {
    let expected_uses = expected_qualification_cache_composite_uses(phase, layer, job)?;
    let mut expected = std::collections::BTreeMap::new();
    expected.insert(
        "qualification_campaign".to_owned(),
        "${{ needs.plan.outputs.qualification_campaign }}".to_owned(),
    );
    expected_qualification_cache_composite_inputs(phase, layer, job, &mut expected)?;
    Ok((expected_uses, expected))
}

fn expected_qualification_cache_composite_uses(
    phase: crate::workflow::step_id::QualificationCacheCompositePhase,
    layer: Option<crate::workflow::qualification_cache_lineage::QualificationCacheLayer>,
    job: &str,
) -> Result<String, ContractError> {
    match (phase, layer) {
        (crate::workflow::step_id::QualificationCacheCompositePhase::Prepare, None) => Ok(format!(
            "./.github/actions/qualification-cache-prepare-{job}"
        )),
        (crate::workflow::step_id::QualificationCacheCompositePhase::BeforeSave, Some(layer)) => {
            Ok(format!(
                "./.github/actions/qualification-cache-before-save-{}-{job}",
                layer.as_str()
            ))
        }
        (crate::workflow::step_id::QualificationCacheCompositePhase::AfterSave, Some(layer)) => {
            Ok(format!(
                "./.github/actions/qualification-cache-after-save-{}-{job}",
                layer.as_str()
            ))
        }
        _ => Err(ContractError::identity(
            "step.id",
            format!("qualification_cache_composite_scope_invalid:{job}"),
        )),
    }
}

fn expected_qualification_cache_composite_inputs(
    phase: crate::workflow::step_id::QualificationCacheCompositePhase,
    layer: Option<crate::workflow::qualification_cache_lineage::QualificationCacheLayer>,
    job: &str,
    expected: &mut std::collections::BTreeMap<String, String>,
) -> Result<(), ContractError> {
    match (phase, layer) {
        (crate::workflow::step_id::QualificationCacheCompositePhase::Prepare, None) => Ok(()),
        (crate::workflow::step_id::QualificationCacheCompositePhase::BeforeSave, Some(_layer)) => {
            let prepare = crate::workflow::step_id::QualificationCacheCompositeId::new(
                crate::workflow::step_id::QualificationCacheCompositePhase::Prepare,
                job,
            )?;
            expected.insert(
                "prepare_outputs".to_owned(),
                format!("${{{{ toJSON(steps.{}.outputs) }}}}", prepare.as_string()),
            );
            Ok(())
        }
        (crate::workflow::step_id::QualificationCacheCompositePhase::AfterSave, Some(layer)) => {
            let prepare = crate::workflow::step_id::QualificationCacheCompositeId::new(
                crate::workflow::step_id::QualificationCacheCompositePhase::Prepare,
                job,
            )?;
            let before = crate::workflow::step_id::QualificationCacheCompositeId::for_layer(
                crate::workflow::step_id::QualificationCacheCompositePhase::BeforeSave,
                layer,
                job,
            )?;
            expected.insert(
                "prepare_outputs".to_owned(),
                format!("${{{{ toJSON(steps.{}.outputs) }}}}", prepare.as_string()),
            );
            expected.insert(
                "before_outputs".to_owned(),
                format!("${{{{ toJSON(steps.{}.outputs) }}}}", before.as_string()),
            );
            let save_id = crate::workflow::step_id::QualificationCacheStepId::new(
                crate::workflow::step_id::QualificationCacheStepStage::Save,
                layer,
                job,
            )?;
            expected.insert(
                format!("cache_{}_save_action_outcome", layer.as_str()),
                format!("${{{{ steps.{}.outcome }}}}", save_id.as_string()),
            );
            Ok(())
        }
        _ => Err(ContractError::identity(
            "step.id",
            format!("qualification_cache_composite_scope_invalid:{job}"),
        )),
    }
}

pub(super) fn qualification_cache_composite_id_matches(
    step: &Step,
    identity: crate::workflow::step_id::QualificationCacheCompositeId,
    kind: &StepKind,
    job: &str,
) -> Result<bool, ContractError> {
    let StepKind::Action { uses, with, env } = kind else {
        return Err(ContractError::identity(
            "step.id",
            format!("id_kind_mismatch:{job}:{identity:?}"),
        ));
    };
    let (expected_uses, expected) =
        expected_qualification_cache_composite_call(identity.phase(), identity.layer(), job)?;
    Ok(uses == &expected_uses
        && with == &expected
        && env.is_empty()
        && step.condition.as_deref() == Some("needs.plan.outputs.qualification_campaign != ''")
        && !step.parallel)
}
