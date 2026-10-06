//! Helpers retain exact source authority while outcomes report after failure.

use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    HelperInvocation, SourceBoundHelper, SourceBoundOperation, StepKind,
};

fn record() -> CompiledSourceHelper {
    let operation = SourceBoundOperation::DesktopNativeHydration;
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let sha = crate::cover_identity::generator::sha256_hex(source.as_bytes());
    let helper = SourceBoundHelper::compiled(operation, operation.path(), &sha).expect("helper");
    let invocation =
        HelperInvocation::compiled(helper, vec!["sdk".into()], Vec::new()).expect("invocation");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("record")
        .with_environment(BTreeMap::from([("HOME".into(), "/owned/home".into())]))
}

fn obligation() -> CrateObligation {
    CrateObligation {
        task_id: "stack/workload/app/build/desktop".into(),
        kind: "build".into(),
        step_name: "Prepare SDK".into(),
        gated_by: Vec::new(),
        matrix_key: "m-0123456789abcdef".into(),
        task_digest: format!("b3-{}", "a".repeat(64)),
        run: vec!["legacy-command-must-not-run".into()],
    }
}

#[test]
fn framing_preserves_owner_authority_and_always_records_actual_outcome() {
    let obligation = obligation();
    let record = record();
    let downstream = vec!["stack/workload/app/test/desktop".into()];
    let steps = steps(
        &obligation,
        &ToolCatalog::pinned(),
        &downstream,
        Some(3),
        &record,
    )
    .expect("framed helper");
    assert_eq!(steps.len(), 3);
    let id = steps[1].id.as_ref().expect("typed ID").as_str();
    assert_eq!(id, "velnor-helper-m-0123456789abcdef");
    let StepKind::SourceBoundHelper { invocation, env } = &steps[1].kind else {
        panic!("source helper required");
    };
    assert_eq!(invocation, record.invocation());
    assert_eq!(env, record.environment());
    assert!(
        steps[1]
            .condition
            .as_deref()
            .expect("gate")
            .starts_with("success()")
    );
    for index in [0, 2] {
        let step = &steps[index];
        assert!(
            step.condition
                .as_deref()
                .expect("gate")
                .starts_with("always()")
        );
        let StepKind::Shell { env, .. } = &step.kind else {
            panic!("report shell required");
        };
        assert_eq!(env[HELPER_ID_ENV], id);
        assert_eq!(env[crate::task_report::DOWNSTREAM_IDS_ENV], downstream[0]);
        assert_eq!(
            env[crate::matrix_step::OBLIGATION_TASK_ID_ENV],
            obligation.task_id
        );
        assert_eq!(
            env[crate::matrix_step::OBLIGATION_TASK_DIGEST_ENV],
            obligation.task_digest
        );
    }
    let StepKind::Shell { env, .. } = &steps[2].kind else {
        panic!("terminal report");
    };
    assert_eq!(
        env[HELPER_OUTCOME_ENV],
        format!("${{{{ steps.{id}.outcome }}}}")
    );
    assert_eq!(env[INTERNAL_OP_ENV], HELPER_REPORT_OP);
}

#[test]
fn planned_descriptor_binds_args_environment_and_source_digest() {
    let record = record();
    let value = descriptor(&record, "m-0123456789abcdef").expect("descriptor");
    assert_eq!(value["id"], "velnor-helper-m-0123456789abcdef");
    assert_eq!(
        value["environment"],
        serde_json::json!(record.environment())
    );
    assert_eq!(value["invocation"], serde_json::json!(record.invocation()));
    assert_eq!(
        value,
        serde_json::to_value(
            HelperObligationDescriptor::from_compiled(&record, "m-0123456789abcdef")
                .expect("typed descriptor"),
        )
        .expect("serialized descriptor"),
    );
    assert!(descriptor(&record, "m-untyped").is_err());
    let mut obligation = obligation();
    obligation.matrix_key = "m-untyped".into();
    assert!(steps(&obligation, &ToolCatalog::pinned(), &[], None, &record).is_err());
}
