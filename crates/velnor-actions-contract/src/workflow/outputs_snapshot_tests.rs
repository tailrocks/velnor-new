use super::{ActionOutput, JobOutput, StepOutputRef, validate_job_outputs};
use crate::workflow::source_helper::{HelperInvocation, SourceBoundHelper, SourceBoundOperation};
use crate::workflow::step::{Step, StepId, StepKind};
use std::collections::BTreeMap;

fn helper_step(id: &str, operation: SourceBoundOperation, args: Vec<String>) -> Step {
    Step {
        id: Some(StepId::new(id).expect("step id")),
        name: "Closed helper".to_owned(),
        condition: None,
        kind: StepKind::SourceBoundHelper {
            invocation: HelperInvocation::compiled(
                SourceBoundHelper::compiled(operation, operation.path(), &"a".repeat(64))
                    .expect("descriptor"),
                args,
                Vec::new(),
            )
            .expect("invocation"),
            env: BTreeMap::new(),
        },
    }
}

fn output(name: &str, step_id: &str, value: ActionOutput) -> JobOutput {
    JobOutput {
        name: name.to_owned(),
        value: StepOutputRef {
            step_id: StepId::new(step_id).expect("step id"),
            output: value,
        },
    }
}

#[test]
fn source_snapshot_outputs_use_exact_names_and_owner() {
    let step = helper_step(
        "snapshot",
        SourceBoundOperation::RustReleaseSourceSnapshot,
        Vec::new(),
    );
    for (index, (value, spelling)) in [
        (
            ActionOutput::SourceSnapshotBlobSha256,
            "source-snapshot-blob-sha256",
        ),
        (ActionOutput::SourceCommitSha, "source-commit-sha"),
        (ActionOutput::SourceTreeSha, "source-tree-sha"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(value.as_str(), spelling);
        let binding = output(&format!("snapshot_{index}"), "snapshot", value);
        assert!(validate_job_outputs(&[binding], std::slice::from_ref(&step)).is_ok());
    }

    let wrong_owner = output(
        "snapshot_wrong_owner",
        "snapshot",
        ActionOutput::SourceIdentity,
    );
    assert!(validate_job_outputs(&[wrong_owner], &[step]).is_err());
}

#[test]
fn source_snapshot_outputs_reject_foreign_or_undeclared_steps() {
    let snapshot_output = output("snapshot", "snapshot", ActionOutput::SourceCommitSha);
    let foreign = helper_step(
        "snapshot",
        SourceBoundOperation::SourceProducerReport,
        Vec::new(),
    );
    assert!(validate_job_outputs(std::slice::from_ref(&snapshot_output), &[foreign]).is_err());
    assert!(validate_job_outputs(&[snapshot_output], &[]).is_err());

    let wrong_args = helper_step(
        "snapshot",
        SourceBoundOperation::MbxProducerReport,
        Vec::new(),
    );
    let wrong_owner = output(
        "snapshot_wrong_operation",
        "snapshot",
        ActionOutput::SourceTreeSha,
    );
    assert!(validate_job_outputs(&[wrong_owner], &[wrong_args]).is_err());
}

#[test]
fn mbx_report_outputs_are_terminal_and_closed() {
    let report = helper_step(
        "report",
        SourceBoundOperation::MbxProducerReport,
        Vec::new(),
    );
    for (index, (value, spelling)) in [
        (ActionOutput::CacheAvailable, "cache_available"),
        (ActionOutput::Verified, "verified"),
        (ActionOutput::SourceIdentity, "sourceidentity"),
        (ActionOutput::Error, "error"),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(value.as_str(), spelling);
        let binding = output(&format!("report_{index}"), "report", value);
        assert!(validate_job_outputs(&[binding], std::slice::from_ref(&report)).is_ok());
    }

    let non_report = helper_step(
        "report",
        SourceBoundOperation::MbxProducerPrepare,
        Vec::new(),
    );
    let binding = output("foreign_report", "report", ActionOutput::Verified);
    assert!(validate_job_outputs(&[binding], &[non_report]).is_err());
}
