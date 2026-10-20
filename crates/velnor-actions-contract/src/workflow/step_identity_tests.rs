use std::collections::BTreeMap;

use super::{
    CACHE_TAR_LAUNCHER_SHA256_INPUT, CACHE_TAR_LAUNCHER_USES, NORMAL_TOOLS_CACHE_EXPORT_OPERATION,
    NORMAL_TOOLS_CACHE_IMPORT_OPERATION, NORMAL_TOOLS_CACHE_STAGE_OPERATION,
    PRESEED_HELPER_SHA256_OUTPUT_EXPR, QualificationCacheCompositeId,
    QualificationCacheCompositePhase, QualificationCacheStepId, QualificationCacheStepStage,
    StepId, StepRole, TOOLS_CACHE_RESTORE_USES, validate_step_sequence,
};
use crate::workflow::{QualificationCacheLayer, Step, StepKind};

fn restore_step(id: Option<StepId>, role: Option<StepRole>, uses: &str) -> Step {
    Step {
        name: "Restore tools cache".to_owned(),
        id,
        role,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn tools_cache_restore_requires_typed_outer_id() {
    let step = restore_step(
        Some(StepId::ToolsRestore),
        Some(StepRole::ToolsCacheRestore),
        TOOLS_CACHE_RESTORE_USES,
    );

    assert!(validate_step_sequence(&[step], "tools-cache").is_ok());
}

#[test]
fn tools_cache_restore_rejects_missing_id_and_wrong_action() {
    let missing_id = restore_step(
        None,
        Some(StepRole::ToolsCacheRestore),
        TOOLS_CACHE_RESTORE_USES,
    );
    let wrong_action = restore_step(
        Some(StepId::ToolsRestore),
        Some(StepRole::ToolsCacheRestore),
        "./.github/actions/other",
    );

    assert!(validate_step_sequence(&[missing_id], "tools-cache").is_err());
    assert!(validate_step_sequence(&[wrong_action], "tools-cache").is_err());
}

#[test]
fn normal_cache_operations_require_registered_ids_and_roles() {
    let cases = [
        (
            StepId::NormalToolsCacheStage,
            StepRole::NormalToolsCacheStage,
            NORMAL_TOOLS_CACHE_STAGE_OPERATION,
            "normal-tools-cache-stage",
        ),
        (
            StepId::NormalToolsCacheImport,
            StepRole::NormalToolsCacheImport,
            NORMAL_TOOLS_CACHE_IMPORT_OPERATION,
            "normal-tools-cache-import",
        ),
        (
            StepId::NormalToolsCacheExport,
            StepRole::NormalToolsCacheExport,
            NORMAL_TOOLS_CACHE_EXPORT_OPERATION,
            "normal-tools-cache-export",
        ),
    ];

    for (id, role, operation, id_text) in cases {
        let step = Step {
            name: "Cache payload operation".to_owned(),
            id: Some(id),
            role: Some(role),
            condition: None,
            parallel: false,
            kind: StepKind::Internal {
                operation: operation.to_owned(),
                env: BTreeMap::new(),
            },
        };
        assert_eq!(id.as_str(), id_text);
        assert!(validate_step_sequence(&[step], "normal-tools-cache").is_ok());
    }
}

#[test]
fn normal_cache_operations_reject_wrong_operation_and_missing_id() {
    let wrong_operation = Step {
        name: "Import tools cache".to_owned(),
        id: Some(StepId::NormalToolsCacheImport),
        role: Some(StepRole::NormalToolsCacheImport),
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: NORMAL_TOOLS_CACHE_EXPORT_OPERATION.to_owned(),
            env: BTreeMap::new(),
        },
    };
    let missing_id = Step {
        name: "Stage tools cache path".to_owned(),
        id: None,
        role: Some(StepRole::NormalToolsCacheStage),
        condition: None,
        parallel: false,
        kind: StepKind::Internal {
            operation: NORMAL_TOOLS_CACHE_STAGE_OPERATION.to_owned(),
            env: BTreeMap::new(),
        },
    };

    assert!(validate_step_sequence(&[wrong_operation], "normal-tools-cache").is_err());
    assert!(validate_step_sequence(&[missing_id], "normal-tools-cache").is_err());
}

#[test]
fn qualification_cache_observation_outputs_have_unique_owner_layer_ids() {
    let layer = QualificationCacheLayer::CargoSources;
    let owner = "rust-example";
    let runtime =
        QualificationCacheStepId::new(QualificationCacheStepStage::RuntimeIdentity, layer, owner)
            .expect("runtime output id");
    let roots = QualificationCacheStepId::new(QualificationCacheStepStage::Roots, layer, owner)
        .expect("roots output id");
    assert_ne!(runtime.as_string(), roots.as_string());

    for (id, role, operation) in [
        (
            runtime,
            StepRole::QualificationRuntimeIdentity,
            "observe-qualification-cache-runtime-v1",
        ),
        (
            roots,
            StepRole::QualificationCacheRoots,
            "observe-qualification-cache-roots-v1",
        ),
    ] {
        let step = Step {
            name: "Observe qualification cache inputs".to_owned(),
            id: Some(StepId::QualificationCache(id)),
            role: Some(role),
            condition: None,
            parallel: false,
            kind: StepKind::Internal {
                operation: operation.to_owned(),
                env: BTreeMap::new(),
            },
        };
        assert!(validate_step_sequence(&[step], owner).is_ok());
    }
}

#[test]
fn qualification_cache_ids_bind_one_job_layer_and_stage() {
    let owner = "rust-example";
    let identity = QualificationCacheStepId::new(
        QualificationCacheStepStage::Restore,
        QualificationCacheLayer::CargoSources,
        owner,
    )
    .expect("valid generated job ID");
    let step = Step {
        name: "Restore qualification sources".to_owned(),
        id: Some(StepId::QualificationCache(identity)),
        role: Some(StepRole::QualificationCacheRestore),
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: "actions/cache/restore@v4".to_owned(),
            with: BTreeMap::from([
                (
                    "key".to_owned(),
                    format!(
                        "${{{{ steps.{}.outputs.restore_key }}}}",
                        identity
                            .for_stage(QualificationCacheStepStage::Bind)
                            .as_string()
                    ),
                ),
                (
                    "path".to_owned(),
                    format!(
                        "${{{{ steps.{}.outputs.restore_stage_path }}}}",
                        identity
                            .for_stage(QualificationCacheStepStage::Bind)
                            .as_string()
                    ),
                ),
            ]),
            env: BTreeMap::new(),
        },
    };

    assert!(
        identity
            .as_string()
            .starts_with("qualification-cache-restore-cargo_sources-")
    );
    assert!(validate_step_sequence(std::slice::from_ref(&step), owner).is_ok());
    assert!(validate_step_sequence(&[step], "different-job").is_err());
}

#[test]
fn qualification_cache_ids_reject_invalid_owner_and_wrong_stage_payload() {
    assert!(
        QualificationCacheStepId::new(
            QualificationCacheStepStage::Restore,
            QualificationCacheLayer::CargoSources,
            "invalid/job",
        )
        .is_err()
    );

    let identity = QualificationCacheStepId::new(
        QualificationCacheStepStage::Save,
        QualificationCacheLayer::MbxObjects,
        "rust-example",
    )
    .expect("valid generated job ID");
    let wrong_payload = Step {
        name: "Wrong qualification cache action".to_owned(),
        id: Some(StepId::QualificationCache(identity)),
        role: Some(StepRole::QualificationCacheSave),
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: "actions/cache/restore@v4".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };

    assert!(validate_step_sequence(&[wrong_payload], "qualification-cache").is_err());
}

#[test]
fn qualification_cache_composite_ids_bind_exact_phase_owner_and_inputs() {
    let owner = "rust-example";
    let prepare =
        QualificationCacheCompositeId::new(QualificationCacheCompositePhase::Prepare, owner)
            .expect("prepare identity");
    let step = Step {
        name: "Prepare qualification cache".to_owned(),
        id: Some(StepId::QualificationCacheComposite(prepare)),
        role: Some(StepRole::QualificationCachePrepareComposite),
        condition: Some("needs.plan.outputs.qualification_campaign != ''".to_owned()),
        parallel: false,
        kind: StepKind::Action {
            uses: format!("./.github/actions/qualification-cache-prepare-{owner}"),
            with: BTreeMap::from([(
                "qualification_campaign".to_owned(),
                "${{ needs.plan.outputs.qualification_campaign }}".to_owned(),
            )]),
            env: BTreeMap::new(),
        },
    };
    assert!(validate_step_sequence(std::slice::from_ref(&step), owner).is_ok());
    assert!(validate_step_sequence(std::slice::from_ref(&step), "other-job").is_err());

    let mut wrong_uses = step.clone();
    let StepKind::Action { uses, .. } = &mut wrong_uses.kind else {
        panic!("composite call is an action");
    };
    *uses = format!("./.github/actions/qualification-cache-finish-{owner}");
    assert!(validate_step_sequence(&[wrong_uses], owner).is_err());

    let mut wrong_input = step;
    let StepKind::Action { with, .. } = &mut wrong_input.kind else {
        panic!("composite call is an action");
    };
    with.insert(
        "arbitrary-output".to_owned(),
        "${{ github.token }}".to_owned(),
    );
    assert!(validate_step_sequence(&[wrong_input], owner).is_err());
}

#[test]
fn cache_tar_launcher_output_has_one_typed_owner() {
    let step = Step {
        name: "Install cache tar adapters".to_owned(),
        id: Some(StepId::CacheTarLaunchers),
        role: Some(StepRole::CacheTarLaunchers),
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: CACHE_TAR_LAUNCHER_USES.to_owned(),
            with: BTreeMap::from([(CACHE_TAR_LAUNCHER_SHA256_INPUT.to_owned(), "a".repeat(64))]),
            env: BTreeMap::new(),
        },
    };

    assert_eq!(StepId::CacheTarLaunchers.as_str(), "cache-tar-launchers");
    assert!(validate_step_sequence(std::slice::from_ref(&step), "cache-tar").is_ok());

    let preseed_digest = Step {
        kind: StepKind::Action {
            uses: CACHE_TAR_LAUNCHER_USES.to_owned(),
            with: BTreeMap::from([(
                CACHE_TAR_LAUNCHER_SHA256_INPUT.to_owned(),
                PRESEED_HELPER_SHA256_OUTPUT_EXPR.to_owned(),
            )]),
            env: BTreeMap::new(),
        },
        ..step.clone()
    };
    assert!(validate_step_sequence(&[preseed_digest], "cache-tar").is_ok());

    let wrong_action = Step {
        kind: StepKind::Action {
            uses: "./.github/actions/other".to_owned(),
            with: BTreeMap::from([(CACHE_TAR_LAUNCHER_SHA256_INPUT.to_owned(), "a".repeat(64))]),
            env: BTreeMap::new(),
        },
        ..step.clone()
    };
    let missing_id = Step {
        id: None,
        ..step.clone()
    };
    assert!(validate_step_sequence(&[wrong_action], "cache-tar").is_err());
    assert!(validate_step_sequence(&[missing_id], "cache-tar").is_err());

    let malformed_digest = Step {
        kind: StepKind::Action {
            uses: CACHE_TAR_LAUNCHER_USES.to_owned(),
            with: BTreeMap::from([(
                CACHE_TAR_LAUNCHER_SHA256_INPUT.to_owned(),
                "not-a-digest".to_owned(),
            )]),
            env: BTreeMap::new(),
        },
        ..step
    };
    assert!(validate_step_sequence(&[malformed_digest], "cache-tar").is_err());
}

#[test]
fn preseed_stage_digest_uses_the_fixed_step_output() {
    let step = Step {
        name: "Stage helper".to_owned(),
        id: Some(StepId::PreseedStage),
        role: Some(StepRole::PreseedStage),
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    };

    assert_eq!(StepId::PreseedStage.as_str(), "preseed-stage");
    assert_eq!(
        PRESEED_HELPER_SHA256_OUTPUT_EXPR,
        "${{ steps.preseed-stage.outputs.helper-sha256 }}"
    );
    assert!(validate_step_sequence(&[step], "preseed").is_ok());
}

#[test]
fn cargo_source_reader_roles_do_not_require_plan_output_ids() {
    let restore = restore_step(
        None,
        Some(StepRole::CargoSourcesRestore),
        "actions/cache/restore@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    let fetch = Step {
        name: "Fetch Cargo sources".to_owned(),
        id: None,
        role: Some(StepRole::CargoSourcesFetch),
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["cargo fetch --locked".to_owned()],
            env: BTreeMap::new(),
        },
    };
    assert!(validate_step_sequence(&[restore], "crate-reader").is_ok());
    assert!(validate_step_sequence(&[fetch], "crate-reader").is_ok());
}
