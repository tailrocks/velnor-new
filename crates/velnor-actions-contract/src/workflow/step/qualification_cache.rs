use super::StepKind;

pub(super) fn qualification_cache_id_matches(
    identity: crate::workflow::step_id::QualificationCacheStepId,
    kind: &StepKind,
) -> bool {
    use crate::workflow::step_id::QualificationCacheStepStage;
    match identity.stage() {
        QualificationCacheStepStage::RuntimeIdentity
        | QualificationCacheStepStage::RuntimeIdentityAfterImport
        | QualificationCacheStepStage::Roots
        | QualificationCacheStepStage::RestoreCensus
        | QualificationCacheStepStage::SaveBeforeCensus
        | QualificationCacheStepStage::SaveAfterCensus
        | QualificationCacheStepStage::Import
        | QualificationCacheStepStage::Export
        | QualificationCacheStepStage::OwnerLayerEvidence
        | QualificationCacheStepStage::OwnerAggregate => {
            matches!(kind, StepKind::Internal { operation, .. }
                if operation == qualification_operation(identity.stage()))
        }
        QualificationCacheStepStage::Bind => matches!(kind,
            StepKind::Internal { operation, .. } if operation == "bind-qualification-cache-v1"),
        QualificationCacheStepStage::Restore => {
            qualification_cache_action_matches(identity, kind, true)
        }
        QualificationCacheStepStage::Save => {
            qualification_cache_action_matches(identity, kind, false)
        }
    }
}

fn qualification_cache_action_matches(
    identity: crate::workflow::step_id::QualificationCacheStepId,
    kind: &StepKind,
    restore: bool,
) -> bool {
    let expected_bind = identity
        .for_stage(crate::workflow::step_id::QualificationCacheStepStage::Bind)
        .as_string();
    let key_output = if restore { "restore_key" } else { "save_key" };
    let path_output = if restore {
        "restore_stage_path"
    } else {
        "save_stage_path"
    };
    matches!(
        kind,
        StepKind::Action { uses, with, .. }
            if uses.starts_with(if restore { "actions/cache/restore@" } else { "actions/cache/save@" })
                && with.get("key") == Some(&format!("${{{{ steps.{expected_bind}.outputs.{key_output} }}}}"))
                && with.get("path") == Some(&format!("${{{{ steps.{expected_bind}.outputs.{path_output} }}}}"))
    )
}

fn qualification_operation(
    stage: crate::workflow::step_id::QualificationCacheStepStage,
) -> &'static str {
    match stage {
        crate::workflow::step_id::QualificationCacheStepStage::RuntimeIdentity
        | crate::workflow::step_id::QualificationCacheStepStage::RuntimeIdentityAfterImport => {
            "observe-qualification-cache-runtime-v1"
        }
        crate::workflow::step_id::QualificationCacheStepStage::Roots => {
            "observe-qualification-cache-roots-v1"
        }
        crate::workflow::step_id::QualificationCacheStepStage::RestoreCensus
        | crate::workflow::step_id::QualificationCacheStepStage::SaveBeforeCensus
        | crate::workflow::step_id::QualificationCacheStepStage::SaveAfterCensus => {
            "observe-qualification-cache-backend-v1"
        }
        crate::workflow::step_id::QualificationCacheStepStage::Import => {
            "import-qualification-cache-payload-v1"
        }
        crate::workflow::step_id::QualificationCacheStepStage::Export => {
            "export-qualification-cache-payload-v1"
        }
        crate::workflow::step_id::QualificationCacheStepStage::OwnerLayerEvidence => {
            "record-qualification-cache-owner-layer-v1"
        }
        crate::workflow::step_id::QualificationCacheStepStage::OwnerAggregate => {
            "aggregate-qualification-cache-owner-v1"
        }
        _ => "",
    }
}
