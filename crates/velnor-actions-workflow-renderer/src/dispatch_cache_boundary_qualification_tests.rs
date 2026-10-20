use super::*;

#[test]
fn full_finalization_accepts_typed_qualification_cache_actions() -> Result<(), crate::RenderError> {
    use velnor_actions_contract::StepRole;
    use velnor_actions_contract::workflow::{
        QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage,
    };

    let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
    let mut ir = dispatch_ir(dispatch_plan_job(&staged)?);
    let owner = PLAN_JOB_ID;
    let layer = QualificationCacheLayer::MbxObjects;
    let bind = QualificationCacheStepId::new(QualificationCacheStepStage::Bind, layer, owner)
        .expect("correct owner");
    append_owner_cache_actions(
        &mut ir.jobs.get_mut(PLAN_JOB_ID).expect("plan job").steps,
        bind,
        layer,
        owner,
    )?;
    crate::cache_doctor::check(&ir.jobs)?;
    let context = dispatch_render_context(staged);
    let jobs = crate::render::finalize_jobs(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
        &dispatch_mise(),
        &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
    )?;
    let plan = &jobs[PLAN_JOB_ID];
    assert!(
        plan.steps
            .iter()
            .any(|step| { step.role == Some(StepRole::QualificationCacheRestore) })
    );
    assert!(
        plan.steps
            .iter()
            .any(|step| { step.role == Some(StepRole::QualificationCacheSave) })
    );
    Ok(())
}

fn append_owner_cache_actions(
    steps: &mut Vec<velnor_actions_contract::Step>,
    bind: velnor_actions_contract::workflow::QualificationCacheStepId,
    layer: velnor_actions_contract::workflow::QualificationCacheLayer,
    owner: &str,
) -> Result<(), crate::RenderError> {
    use velnor_actions_contract::StepRole;
    use velnor_actions_contract::workflow::{
        QualificationCacheStepId, QualificationCacheStepStage, StepId,
    };

    let bind_id = bind.as_string();
    let restore = QualificationCacheStepId::new(QualificationCacheStepStage::Restore, layer, owner)
        .expect("correct restore ID");
    let restore_key = format!("${{{{ steps.{bind_id}.outputs.restore_key }}}}");
    let restore_path = format!("${{{{ steps.{bind_id}.outputs.restore_stage_path }}}}");
    let uses = velnor_actions_actionlint::PinnedActionRef::new(
        "actions/cache",
        Some("restore"),
        velnor_actions_actionlint::actions::CACHE_ACTION_SHA,
        velnor_actions_actionlint::actions::CACHE_ACTION_VERSION,
    )
    .expect("correct cache restore action")
    .uses_value();
    let mut restore_step = crate::cache_steps::cache_action_step(
        true,
        &uses,
        cache_action_layer_name(layer),
        &restore_key,
        &[],
        &[restore_path],
    )?;
    restore_step.id = Some(StepId::QualificationCache(restore));
    restore_step.role = Some(StepRole::QualificationCacheRestore);
    restore_step.condition = Some(format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{bind_id}.outputs.active == 'true'"
    ));
    steps.push(restore_step);

    let save = QualificationCacheStepId::new(QualificationCacheStepStage::Save, layer, owner)
        .expect("correct save ID");
    let export_id =
        QualificationCacheStepId::new(QualificationCacheStepStage::Export, layer, owner)
            .expect("correct export ID")
            .as_string();
    let save_key = format!("${{{{ steps.{bind_id}.outputs.save_key }}}}");
    let save_path = format!("${{{{ steps.{bind_id}.outputs.save_stage_path }}}}");
    let uses = velnor_actions_actionlint::PinnedActionRef::new(
        "actions/cache",
        Some("save"),
        velnor_actions_actionlint::actions::CACHE_ACTION_SHA,
        velnor_actions_actionlint::actions::CACHE_ACTION_VERSION,
    )
    .expect("correct cache save action")
    .uses_value();
    let mut save_step = crate::cache_steps::cache_action_step(
        false,
        &uses,
        cache_action_layer_name(layer),
        &save_key,
        &[],
        &[save_path],
    )?;
    save_step.id = Some(StepId::QualificationCache(save));
    save_step.role = Some(StepRole::QualificationCacheSave);
    save_step.condition = Some(format!(
        "needs.plan.outputs.qualification_campaign != '' && steps.{bind_id}.outputs.active == 'true' && steps.{export_id}.outputs.save_requested == 'true' && steps.{bind_id}.outputs.save_key != ''"
    ));
    steps.push(save_step);
    Ok(())
}

fn cache_action_layer_name(
    layer: velnor_actions_contract::workflow::QualificationCacheLayer,
) -> &'static str {
    match layer {
        velnor_actions_contract::workflow::QualificationCacheLayer::MbxObjects => "mbx-objects",
        velnor_actions_contract::workflow::QualificationCacheLayer::CargoSources => "sources",
        velnor_actions_contract::workflow::QualificationCacheLayer::MiseTools => "tools",
        velnor_actions_contract::workflow::QualificationCacheLayer::TofuProviders => {
            "tofu-providers"
        }
        velnor_actions_contract::workflow::QualificationCacheLayer::TaskResult => "task",
    }
}
