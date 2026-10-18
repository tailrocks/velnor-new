use std::collections::BTreeMap;

use super::{
    NORMAL_TOOLS_CACHE_EXPORT_OPERATION, NORMAL_TOOLS_CACHE_IMPORT_OPERATION,
    NORMAL_TOOLS_CACHE_STAGE_OPERATION, StepId, StepRole, TOOLS_CACHE_RESTORE_USES,
    validate_step_sequence,
};
use crate::workflow::{Step, StepKind};

fn restore_step(id: Option<StepId>, role: Option<StepRole>, uses: &str) -> Step {
    Step {
        name: "Restore tools cache".to_owned(),
        id,
        role,
        condition: None,
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
        Some(StepId::ToolsCacheRestore),
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
        Some(StepId::ToolsCacheRestore),
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
        kind: StepKind::Internal {
            operation: NORMAL_TOOLS_CACHE_STAGE_OPERATION.to_owned(),
            env: BTreeMap::new(),
        },
    };

    assert!(validate_step_sequence(&[wrong_operation], "normal-tools-cache").is_err());
    assert!(validate_step_sequence(&[missing_id], "normal-tools-cache").is_err());
}
