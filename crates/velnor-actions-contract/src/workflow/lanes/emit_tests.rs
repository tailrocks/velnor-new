use std::collections::BTreeMap;

use super::*;
use crate::workflow::{
    QualificationCacheCompositeId, QualificationCacheCompositePhase, QualificationCacheLayer,
    QualificationCacheStepId, QualificationCacheStepStage, Step, StepId, StepKind, StepRole,
};

const SOURCE_OWNER: &str = "rust-demo";
const HOSTED_OWNER: &str = "rust-demo__hosted";
const SCALE_OWNER: &str = "rust-demo__local";
const OWNER_ENV: &str = "VELNOR_QUALIFICATION_CACHE_OWNER_ID";

fn owner_job(owner_id: &str) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Rust demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: crate::workflow::JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: cache_owner_steps(owner_id),
    }
}

fn cache_owner_steps(owner_id: &str) -> Vec<Step> {
    let layer = QualificationCacheLayer::CargoSources;
    let bind = QualificationCacheStepId::new(QualificationCacheStepStage::Bind, layer, owner_id)
        .expect("valid cache owner");
    let restore =
        QualificationCacheStepId::new(QualificationCacheStepStage::Restore, layer, owner_id)
            .expect("valid cache owner");
    let composite =
        QualificationCacheCompositeId::new(QualificationCacheCompositePhase::Prepare, owner_id)
            .expect("valid cache owner");
    vec![
        Step {
            name: "Bind cache owner".to_owned(),
            id: Some(StepId::QualificationCache(bind)),
            role: Some(StepRole::QualificationCacheBind),
            condition: Some(format!("steps.{}.outcome == 'success'", bind.as_string())),
            parallel: false,
            kind: StepKind::Internal {
                operation: "bind-qualification-cache-v1".to_owned(),
                env: BTreeMap::from([
                    (OWNER_ENV.to_owned(), owner_id.to_owned()),
                    (
                        "VELNOR_QUALIFICATION_CACHE_LAYER".to_owned(),
                        layer.as_str().to_owned(),
                    ),
                ]),
            },
        },
        Step {
            name: "Restore cache".to_owned(),
            id: Some(StepId::QualificationCache(restore)),
            role: Some(StepRole::QualificationCacheRestore),
            condition: None,
            parallel: false,
            kind: StepKind::Action {
                uses: "actions/cache/restore@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                with: BTreeMap::from([
                    (
                        "key".to_owned(),
                        format!("${{{{ steps.{}.outputs.restore_key }}}}", bind.as_string()),
                    ),
                    (
                        "path".to_owned(),
                        format!(
                            "${{{{ steps.{}.outputs.restore_stage_path }}}}",
                            bind.as_string()
                        ),
                    ),
                ]),
                env: BTreeMap::from([
                    (OWNER_ENV.to_owned(), owner_id.to_owned()),
                    (
                        "CACHE_OWNER_REFERENCE".to_owned(),
                        format!("${{{{ steps.{}.outcome }}}}", bind.as_string()),
                    ),
                ]),
            },
        },
        Step {
            name: "Composite cache operation".to_owned(),
            id: Some(StepId::QualificationCacheComposite(composite)),
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Internal {
                operation: "composite-cache-operation-v1".to_owned(),
                env: BTreeMap::from([(
                    "CACHE_BIND_REFERENCE".to_owned(),
                    format!("${{{{ steps.{}.outputs.result }}}}", composite.as_string()),
                )]),
            },
        },
    ]
}

fn expand_owner(job: &Job, output_id: &str, lane: EmitLane) -> Job {
    retarget(
        job,
        None,
        SOURCE_OWNER,
        output_id,
        LaneClass::Verification,
        lane,
        &BTreeMap::new(),
    )
    .expect("the original cache owner must retarget to its emitted job ID")
}

fn assert_owner(job: &Job, owner_id: &str) {
    let bind = QualificationCacheStepId::new(
        QualificationCacheStepStage::Bind,
        QualificationCacheLayer::CargoSources,
        owner_id,
    )
    .expect("valid emitted cache owner");
    let restore = QualificationCacheStepId::new(
        QualificationCacheStepStage::Restore,
        QualificationCacheLayer::CargoSources,
        owner_id,
    )
    .expect("valid emitted cache owner");
    let composite =
        QualificationCacheCompositeId::new(QualificationCacheCompositePhase::Prepare, owner_id)
            .expect("valid emitted cache owner");
    let StepKind::Internal { env, .. } = &job.steps[0].kind else {
        panic!("the bind step must remain an internal operation");
    };
    assert_eq!(env.get(OWNER_ENV), Some(&owner_id.to_owned()));
    assert_eq!(job.steps[0].id, Some(StepId::QualificationCache(bind)));
    assert_eq!(
        job.steps[0].condition.as_deref(),
        Some(format!("steps.{}.outcome == 'success'", bind.as_string()).as_str())
    );
    assert_eq!(job.steps[1].id, Some(StepId::QualificationCache(restore)));
    assert_eq!(
        job.steps[2].id,
        Some(StepId::QualificationCacheComposite(composite))
    );
    let StepKind::Internal { env, .. } = &job.steps[2].kind else {
        panic!("the composite owner must remain an internal operation");
    };
    assert_eq!(
        env.get("CACHE_BIND_REFERENCE"),
        Some(&format!(
            "${{{{ steps.{}.outputs.result }}}}",
            composite.as_string()
        ))
    );
    let StepKind::Action { with, env, .. } = &job.steps[1].kind else {
        panic!("the restore step must remain a cache action");
    };
    assert_eq!(env.get(OWNER_ENV), Some(&owner_id.to_owned()));
    assert_eq!(
        env.get("CACHE_OWNER_REFERENCE"),
        Some(&format!("${{{{ steps.{}.outcome }}}}", bind.as_string()))
    );
    assert_eq!(
        with.get("key"),
        Some(&format!(
            "${{{{ steps.{}.outputs.restore_key }}}}",
            bind.as_string()
        ))
    );
    assert_eq!(
        with.get("path"),
        Some(&format!(
            "${{{{ steps.{}.outputs.restore_stage_path }}}}",
            bind.as_string()
        ))
    );
}

#[test]
fn both_lanes_rebind_cache_owner_ids_and_all_generated_references() {
    let original = owner_job(SOURCE_OWNER);
    let hosted = expand_owner(&original, HOSTED_OWNER, EmitLane::Hosted);
    let scale = expand_owner(&original, SCALE_OWNER, EmitLane::Scale);

    assert_owner(&hosted, HOSTED_OWNER);
    assert_owner(&scale, SCALE_OWNER);
}

#[test]
fn single_lane_modes_keep_their_original_cache_owner_and_references() {
    let original = owner_job(SOURCE_OWNER);
    let hosted = expand_owner(&original, SOURCE_OWNER, EmitLane::Single);
    let scale = retarget(
        &original,
        Some("ubuntu-26.04-scale-set"),
        SOURCE_OWNER,
        SOURCE_OWNER,
        LaneClass::Verification,
        EmitLane::Single,
        &BTreeMap::new(),
    )
    .expect("the Scale Set owner remains the same");

    let original_bytes = serde_json::to_vec(&original).expect("the job serializes");
    assert_eq!(
        serde_json::to_vec(&hosted).expect("the hosted job serializes"),
        original_bytes
    );
    assert_eq!(
        serde_json::to_vec(&scale).expect("the Scale Set job serializes"),
        serde_json::to_vec(&Job {
            runs_on: "ubuntu-26.04-scale-set".to_owned(),
            ..original.clone()
        })
        .expect("the expected Scale Set job serializes")
    );
    assert_owner(&hosted, SOURCE_OWNER);
    assert_owner(&scale, SOURCE_OWNER);
}

#[test]
fn mismatched_cache_identity_owner_is_rejected_before_retargeting() {
    let mut original = owner_job(SOURCE_OWNER);
    let wrong_bind = QualificationCacheStepId::new(
        QualificationCacheStepStage::Bind,
        QualificationCacheLayer::CargoSources,
        "other-owner",
    )
    .expect("valid unrelated cache owner");
    original.steps[0].id = Some(StepId::QualificationCache(wrong_bind));
    let before = serde_json::to_vec(&original).expect("the job serializes");

    let result = retarget(
        &original,
        Some("scale-set-token"),
        SOURCE_OWNER,
        HOSTED_OWNER,
        LaneClass::Verification,
        EmitLane::Hosted,
        &BTreeMap::new(),
    );

    assert!(matches!(result, Err(ContractError::InvalidIdentity { .. })));
    assert_eq!(
        serde_json::to_vec(&original).expect("the job still serializes"),
        before
    );
}

#[test]
fn mismatched_composite_cache_owner_is_rejected_before_retargeting() {
    let mut original = owner_job(SOURCE_OWNER);
    let wrong_composite = QualificationCacheCompositeId::new(
        QualificationCacheCompositePhase::Prepare,
        "other-owner",
    )
    .expect("valid unrelated cache owner");
    original.steps[2].id = Some(StepId::QualificationCacheComposite(wrong_composite));
    let before = serde_json::to_vec(&original).expect("the job serializes");

    let result = retarget(
        &original,
        None,
        SOURCE_OWNER,
        HOSTED_OWNER,
        LaneClass::Verification,
        EmitLane::Hosted,
        &BTreeMap::new(),
    );

    assert!(matches!(result, Err(ContractError::InvalidIdentity { .. })));
    assert_eq!(
        serde_json::to_vec(&original).expect("the job still serializes"),
        before
    );
}

#[test]
fn mismatched_cache_owner_environment_is_rejected() {
    let mut original = owner_job(SOURCE_OWNER);
    let StepKind::Internal { env, .. } = &mut original.steps[0].kind else {
        panic!("the bind step must remain an internal operation");
    };
    env.insert(OWNER_ENV.to_owned(), "other-owner".to_owned());

    let result = retarget(
        &original,
        None,
        SOURCE_OWNER,
        HOSTED_OWNER,
        LaneClass::Verification,
        EmitLane::Hosted,
        &BTreeMap::new(),
    );

    assert!(matches!(result, Err(ContractError::InvalidIdentity { .. })));
}

#[path = "emit_owner_tests.rs"]
mod owner;
