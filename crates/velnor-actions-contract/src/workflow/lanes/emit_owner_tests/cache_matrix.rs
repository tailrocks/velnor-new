use super::*;

pub(super) fn cache_layers() -> [QualificationCacheLayer; 5] {
    [
        QualificationCacheLayer::MbxObjects,
        QualificationCacheLayer::CargoSources,
        QualificationCacheLayer::MiseTools,
        QualificationCacheLayer::TofuProviders,
        QualificationCacheLayer::TaskResult,
    ]
}

pub(super) fn cache_stages() -> [QualificationCacheStepStage; 13] {
    [
        QualificationCacheStepStage::Bind,
        QualificationCacheStepStage::Restore,
        QualificationCacheStepStage::Save,
        QualificationCacheStepStage::RuntimeIdentity,
        QualificationCacheStepStage::Roots,
        QualificationCacheStepStage::RuntimeIdentityAfterImport,
        QualificationCacheStepStage::RestoreCensus,
        QualificationCacheStepStage::Import,
        QualificationCacheStepStage::Export,
        QualificationCacheStepStage::SaveBeforeCensus,
        QualificationCacheStepStage::SaveAfterCensus,
        QualificationCacheStepStage::OwnerLayerEvidence,
        QualificationCacheStepStage::OwnerAggregate,
    ]
}

pub(super) fn append_cache_reference_steps(
    job: &mut Job,
    layers: &[QualificationCacheLayer],
    stages: &[QualificationCacheStepStage],
) {
    for layer in layers {
        for stage in stages {
            append_cache_stage_step(job, *stage, *layer);
        }
        for phase in [
            QualificationCacheCompositePhase::BeforeSave,
            QualificationCacheCompositePhase::AfterSave,
        ] {
            append_layer_composite_step(job, phase, *layer);
        }
    }
    append_owner_composite_step(job);
}

pub(super) fn append_cache_stage_step(
    job: &mut Job,
    stage: QualificationCacheStepStage,
    layer: QualificationCacheLayer,
) {
    let identity = QualificationCacheStepId::new(stage, layer, SOURCE_OWNER)
        .expect("valid generated cache identity");
    let reference = identity.as_string();
    job.steps.push(Step {
        name: "Generated cache reference".to_owned(),
        id: Some(StepId::QualificationCache(identity)),
        role: None,
        condition: Some(format!("steps.{reference}.outcome == 'success'")),
        parallel: false,
        kind: StepKind::Shell {
            run: vec![format!("echo ${{{{ steps.{reference}.outcome }}}}")],
            env: BTreeMap::new(),
        },
    });
}

pub(super) fn append_layer_composite_step(
    job: &mut Job,
    phase: QualificationCacheCompositePhase,
    layer: QualificationCacheLayer,
) {
    let identity = QualificationCacheCompositeId::for_layer(phase, layer, SOURCE_OWNER)
        .expect("valid generated cache composite");
    job.steps.push(Step {
        name: "Generated cache aggregate".to_owned(),
        id: Some(StepId::QualificationCacheComposite(identity)),
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: "cache-aggregate-test".to_owned(),
            env: BTreeMap::from([(
                "CACHE_AGGREGATE_REFERENCE".to_owned(),
                format!("${{{{ steps.{}.outputs.result }}}}", identity.as_string()),
            )]),
        },
    });
}

pub(super) fn append_owner_composite_step(job: &mut Job) {
    let identity =
        QualificationCacheCompositeId::new(QualificationCacheCompositePhase::Prepare, SOURCE_OWNER)
            .expect("valid generated owner prepare composite");
    job.steps.push(Step {
        name: "Generated cache owner aggregate".to_owned(),
        id: Some(StepId::QualificationCacheComposite(identity)),
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: "cache-owner-aggregate-test".to_owned(),
            env: BTreeMap::from([(
                "CACHE_AGGREGATE_REFERENCE".to_owned(),
                format!("${{{{ steps.{}.outputs.result }}}}", identity.as_string()),
            )]),
        },
    });
}

pub(super) fn assert_cache_references_retargeted(
    job: &Job,
    layer_count: usize,
    stage_count: usize,
) {
    assert_eq!(
        job.steps
            .iter()
            .filter(|step| matches!(step.id, Some(StepId::QualificationCache(_))))
            .count(),
        2 + layer_count * stage_count
    );
    assert_eq!(
        job.steps
            .iter()
            .filter(|step| matches!(step.id, Some(StepId::QualificationCacheComposite(_))))
            .count(),
        2 + layer_count * 2
    );
    for step in job.steps[3..]
        .iter()
        .filter(|step| matches!(step.id, Some(StepId::QualificationCache(_))))
    {
        assert_cache_stage_retargeted(step);
    }
    for step in job.steps[3..]
        .iter()
        .filter(|step| matches!(step.id, Some(StepId::QualificationCacheComposite(_))))
    {
        assert_cache_composite_retargeted(step);
    }
}

pub(super) fn assert_cache_stage_retargeted(step: &Step) {
    let Some(StepId::QualificationCache(identity)) = step.id else {
        panic!("each generated cache step must retain its typed identity");
    };
    assert!(identity.belongs_to(HOSTED_OWNER));
    assert_eq!(
        step.condition.as_deref(),
        Some(format!("steps.{}.outcome == 'success'", identity.as_string()).as_str())
    );
    let StepKind::Shell { run, .. } = &step.kind else {
        panic!("generated cache step must remain a shell step");
    };
    assert_eq!(
        run[0],
        format!("echo ${{{{ steps.{}.outcome }}}}", identity.as_string())
    );
}

pub(super) fn assert_cache_composite_retargeted(step: &Step) {
    let Some(StepId::QualificationCacheComposite(identity)) = step.id else {
        panic!("each generated aggregate must retain its typed identity");
    };
    assert!(identity.belongs_to(HOSTED_OWNER));
    let StepKind::Internal { env, .. } = &step.kind else {
        panic!("generated aggregate must remain an internal operation");
    };
    assert_eq!(
        env["CACHE_AGGREGATE_REFERENCE"],
        format!("${{{{ steps.{}.outputs.result }}}}", identity.as_string())
    );
}

pub(super) fn assert_unknown_cache_reference_fails() {
    let mut job = owner_job(SOURCE_OWNER);
    job.steps.push(Step {
        name: "Unknown cache output".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["echo ${{ steps.qualification-cache-missing.outputs.value }}".to_owned()],
            env: BTreeMap::new(),
        },
    });
    let result = retarget(
        &job,
        None,
        SOURCE_OWNER,
        HOSTED_OWNER,
        LaneClass::Verification,
        EmitLane::Hosted,
        &BTreeMap::new(),
    );
    assert!(matches!(result, Err(ContractError::InvalidIdentity { .. })));
}

#[test]
fn single_quoted_backslash_does_not_hide_later_cache_references() {
    let bind = bind_id(SOURCE_OWNER);
    let hosted_bind = bind_id(HOSTED_OWNER);
    let expression = format!(r"${{{{ 'C:\' != '' && steps.{bind}.outputs.restore_key }}}}");
    let condition = format!(r"'C:\' != '' && steps.{bind}.outputs.restore_key");
    let mut job = owner_job(SOURCE_OWNER);
    job.steps.push(Step {
        name: "Single quote after backslash".to_owned(),
        id: None,
        role: None,
        condition: Some(condition),
        parallel: false,
        kind: StepKind::Shell {
            run: vec![expression],
            env: BTreeMap::new(),
        },
    });

    let retargeted = expand_owner(&job, HOSTED_OWNER, EmitLane::Hosted);
    let step = retargeted.steps.last().expect("the fixture step remains");
    let StepKind::Shell { run, .. } = &step.kind else {
        panic!("the fixture remains a shell step");
    };
    assert_eq!(
        run[0],
        format!(r"${{{{ 'C:\' != '' && steps.{hosted_bind}.outputs.restore_key }}}}")
    );
    assert_eq!(
        step.condition.as_deref(),
        Some(format!(r"'C:\' != '' && steps.{hosted_bind}.outputs.restore_key").as_str())
    );
}

#[test]
fn whitespace_before_nested_steps_does_not_retarget_event_properties() {
    let bind = bind_id(SOURCE_OWNER);
    let hosted_bind = bind_id(HOSTED_OWNER);
    let nested = format!("github.event. steps.{bind}.outputs.restore_key");
    let reference = format!("steps.{bind}.outputs.restore_key");
    let nested_hosted = format!("github.event. steps.{bind}.outputs.restore_key");
    let reference_hosted = format!("steps.{hosted_bind}.outputs.restore_key");
    let mut job = owner_job(SOURCE_OWNER);
    job.steps.push(Step {
        name: "Whitespace before nested property".to_owned(),
        id: None,
        role: None,
        condition: Some(format!("{nested} || {reference}")),
        parallel: false,
        kind: StepKind::Shell {
            run: vec![format!("${{{{ {nested} || {reference} }}}}")],
            env: BTreeMap::new(),
        },
    });

    let retargeted = expand_owner(&job, HOSTED_OWNER, EmitLane::Hosted);
    let step = retargeted.steps.last().expect("the fixture step remains");
    let StepKind::Shell { run, .. } = &step.kind else {
        panic!("the fixture remains a shell step");
    };
    assert_eq!(
        run[0],
        format!("${{{{ {nested_hosted} || {reference_hosted} }}}}")
    );
    assert_eq!(
        step.condition.as_deref(),
        Some(format!("{nested_hosted} || {reference_hosted}").as_str())
    );
}
