use super::*;

#[test]
fn cold_and_warm_dispatch_keep_bound_qualification_cache_actions() -> Result<(), crate::RenderError>
{
    use velnor_actions_contract::StepRole;
    use velnor_actions_contract::workflow::{
        QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage,
    };

    for phase in ["cold", "warm"] {
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
        let dispatch = ir
            .triggers
            .workflow_dispatch
            .as_mut()
            .expect("The workflow dispatch trigger is missing.");
        dispatch.inputs[0].default = Some(phase.to_owned());
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
        let bind_id = bind.as_string();
        let export_id =
            QualificationCacheStepId::new(QualificationCacheStepStage::Export, layer, owner)
                .expect("The test export ID is invalid.")
                .as_string();
        let restore = plan
            .steps
            .iter()
            .find(|step| step.role == Some(StepRole::QualificationCacheRestore))
            .expect("The typed restore action is missing.");
        let save = plan
            .steps
            .iter()
            .find(|step| step.role == Some(StepRole::QualificationCacheSave))
            .expect("The typed save action is missing.");
        assert_eq!(
            restore.condition.as_deref(),
            Some(
                format!("needs.plan.outputs.qualification_campaign != '' && steps.{bind_id}.outputs.active == 'true'")
                    .as_str()
            )
        );
        assert_eq!(
            save.condition.as_deref(),
            Some(
                format!("needs.plan.outputs.qualification_campaign != '' && steps.{bind_id}.outputs.active == 'true' && steps.{export_id}.outputs.save_requested == 'true' && steps.{bind_id}.outputs.save_key != ''")
                    .as_str()
            )
        );
    }
    Ok(())
}

#[test]
fn role_only_or_unguarded_qualification_cache_actions_stay_denied() -> Result<(), crate::RenderError>
{
    use velnor_actions_contract::StepRole;
    use velnor_actions_contract::workflow::{
        QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage,
    };

    let layer = QualificationCacheLayer::MbxObjects;
    let owner = PLAN_JOB_ID;
    let bind = QualificationCacheStepId::new(QualificationCacheStepStage::Bind, layer, owner)
        .expect("correct owner");
    let mut steps = Vec::new();
    append_owner_cache_actions(&mut steps, bind, layer, owner)?;
    let restore_index = steps
        .iter()
        .position(|step| step.role == Some(StepRole::QualificationCacheRestore))
        .expect("The restore action is missing.");
    let save_index = steps
        .iter()
        .position(|step| step.role == Some(StepRole::QualificationCacheSave))
        .expect("The save action is missing.");

    for (index, invalid) in [
        (restore_index, "missing"),
        (restore_index, "wrong"),
        (restore_index, "unbound-key"),
        (restore_index, "role-only"),
        (restore_index, "wrong-stage"),
        (restore_index, "parallel"),
        (save_index, "missing-save-guard"),
        (save_index, "wrong-save-key"),
    ] {
        assert_cache_step_denied(steps.clone(), index, invalid, layer, owner);
    }

    Ok(())
}

fn assert_cache_step_denied(
    mut steps: Vec<velnor_actions_contract::Step>,
    index: usize,
    invalid: &str,
    layer: velnor_actions_contract::workflow::QualificationCacheLayer,
    owner: &str,
) {
    match invalid {
        "missing" | "missing-save-guard" => steps[index].condition = None,
        "wrong" => steps[index].condition = Some("always()".to_owned()),
        "unbound-key" | "wrong-save-key" => {
            let velnor_actions_contract::StepKind::Action { with, .. } = &mut steps[index].kind
            else {
                panic!("The typed cache step must use an action.");
            };
            with.insert("key".to_owned(), "${{ inputs.cache_key }}".to_owned());
        }
        "role-only" => steps[index].id = None,
        "parallel" => steps[index].parallel = true,
        "wrong-stage" => {
            steps[index].id = Some(
                velnor_actions_contract::workflow::StepId::QualificationCache(
                    velnor_actions_contract::workflow::QualificationCacheStepId::new(
                        velnor_actions_contract::workflow::QualificationCacheStepStage::Save,
                        layer,
                        owner,
                    )
                    .expect("The test save ID is invalid."),
                ),
            );
        }
        _ => unreachable!(),
    }
    let mut jobs = std::collections::BTreeMap::from([(
        "plan".to_owned(),
        velnor_actions_contract::Job {
            services: Vec::new(),
            display_name: "plan".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: velnor_actions_contract::JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps,
            cache_mode: None,
        },
    )]);
    super::suppress_unvalidated_cache_access(&mut jobs);
    assert!(
        jobs["plan"].steps[index]
            .condition
            .as_deref()
            .is_some_and(|condition| condition.ends_with(super::DISPATCH_DENY))
    );
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
        .expect("The test save ID is invalid.");
    let export_id =
        QualificationCacheStepId::new(QualificationCacheStepStage::Export, layer, owner)
            .expect("The test export ID is invalid.")
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
