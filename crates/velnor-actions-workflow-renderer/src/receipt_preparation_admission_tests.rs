use super::validate_steps;
use crate::source_helper::{source_helper_step, step_to_yaml};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation, StepKind,
};

fn record(operation: SourceBoundOperation) -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let invocation =
        HelperInvocation::compiled(descriptor, Vec::new(), Vec::new()).expect("invocation");
    CompiledSourceHelper::compiled(invocation, source).expect("record")
}

#[test]
fn registry_record_does_not_mint_receipt_transport_authority() {
    let record = record(SourceBoundOperation::ReceiptOwnedPreparation);
    let step = source_helper_step("Receipt preparation", &record, BTreeMap::new()).expect("step");
    let error = validate_steps(std::iter::once(&step)).expect_err("missing owned binding");
    assert!(
        error
            .to_string()
            .contains("receipt_preparation_requires_owned_binding")
    );
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        panic!("helper");
    };
    let error = step_to_yaml(
        &step,
        invocation,
        env,
        std::slice::from_ref(&record),
        "ubuntu-26.04",
    )
    .expect_err("direct serialization cannot bypass binding");
    assert!(
        error
            .to_string()
            .contains("receipt_preparation_requires_owned_binding")
    );
}

#[test]
fn ordinary_registered_helpers_preserve_admission() {
    let record = record(SourceBoundOperation::RustPrepareRootLinux);
    let step = source_helper_step("Prepare", &record, BTreeMap::new()).expect("step");
    assert!(validate_steps(std::iter::once(&step)).is_ok());
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        panic!("helper");
    };
    assert!(
        step_to_yaml(
            &step,
            invocation,
            env,
            std::slice::from_ref(&record),
            "ubuntu-26.04",
        )
        .is_ok()
    );
}
