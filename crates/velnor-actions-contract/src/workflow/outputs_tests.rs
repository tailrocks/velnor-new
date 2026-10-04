use super::{ActionOutput, JobOutput, StepOutputRef, validate_job_outputs};
use crate::workflow::step::{Step, StepId, StepKind};
use std::collections::BTreeMap;

fn step() -> Step {
    Step {
        id: Some(StepId::new("upload").expect("step")),
        name: "Upload artifact".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: format!("actions/upload-artifact@{}", "a".repeat(40)),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn output() -> JobOutput {
    JobOutput {
        name: "artifact_id".to_owned(),
        value: StepOutputRef {
            step_id: StepId::new("upload").expect("step"),
            output: ActionOutput::ArtifactId,
        },
    }
}

#[test]
fn native_outputs_require_known_declared_action() {
    let valid = output();
    assert!(validate_job_outputs(std::slice::from_ref(&valid), &[step()]).is_ok());
    assert_eq!(
        valid.value.expression(),
        "${{ steps.upload.outputs.artifact-id }}"
    );
    assert!(validate_job_outputs(std::slice::from_ref(&valid), &[]).is_err());
    let mut wrong = step();
    wrong.id = None;
    assert!(validate_job_outputs(std::slice::from_ref(&valid), &[wrong.clone()]).is_err());
    wrong.id = Some(StepId::new("upload").expect("step"));
    wrong.kind = StepKind::Internal {
        operation: "upload".to_owned(),
    };
    assert!(validate_job_outputs(std::slice::from_ref(&valid), &[wrong]).is_err());
    let mut wrong = valid;
    wrong.value.output = ActionOutput::PageUrl;
    assert!(validate_job_outputs(&[wrong], &[step()]).is_err());
    assert!(serde_json::from_str::<ActionOutput>("\"unknown\"").is_err());
}

#[test]
fn native_outputs_reject_duplicate_and_invalid_names() {
    let valid = output();
    assert!(validate_job_outputs(&[valid.clone(), valid.clone()], &[step()]).is_err());
    for name in ["", "a.b", "${{ x }}", "1start", "a\n"] {
        let mut bad = valid.clone();
        bad.name = name.to_owned();
        assert!(validate_job_outputs(&[bad], &[step()]).is_err());
    }
    for uses in [
        "actions/upload-artifact@v4",
        "actions/download-artifact@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let mut bad = step();
        if let StepKind::Action { uses: action, .. } = &mut bad.kind {
            *action = uses.to_owned();
        }
        assert!(validate_job_outputs(std::slice::from_ref(&valid), &[bad]).is_err());
    }
}

fn helper(
    operation: crate::workflow::source_helper::SourceBoundOperation,
    args: Vec<String>,
) -> Step {
    use crate::workflow::source_helper::{HelperInvocation, SourceBoundHelper};
    Step {
        id: Some(StepId::new("upload").expect("step")),
        name: "Report".to_owned(),
        condition: None,
        kind: StepKind::SourceBoundHelper {
            invocation: HelperInvocation::compiled(
                SourceBoundHelper::compiled(operation, operation.path(), &"a".repeat(64))
                    .expect("binding"),
                args,
                Vec::new(),
            )
            .expect("invocation"),
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn native_helper_outputs_match_closed_report_owners() {
    use crate::workflow::source_helper::SourceBoundOperation::{
        SourceProducerReport, ToolProducerReport,
    };
    let mut output = output();
    for (name, source, tool) in [
        (ActionOutput::CacheAvailable, true, true),
        (ActionOutput::Verified, true, true),
        (ActionOutput::Error, true, true),
        (ActionOutput::SourceIdentity, true, false),
        (ActionOutput::ToolIdentity, false, true),
        (ActionOutput::DescriptorIdentity, false, true),
        (ActionOutput::ArtifactDigest, false, false),
    ] {
        output.value.output = name;
        assert_eq!(
            validate_job_outputs(
                std::slice::from_ref(&output),
                &[helper(SourceProducerReport, Vec::new())]
            )
            .is_ok(),
            source
        );
        assert_eq!(
            validate_job_outputs(
                std::slice::from_ref(&output),
                &[helper(ToolProducerReport, Vec::new())]
            )
            .is_ok(),
            tool
        );
    }
}

#[test]
fn native_oci_digest_is_not_an_archive_digest_or_arbitrary_phase() {
    use crate::workflow::source_helper::SourceBoundOperation::{
        NativePublishReceiptVerifier, OciDelivery,
    };
    let mut output = output();
    output.value.output = ActionOutput::OciIndexDigest;
    for phase in [
        "admission",
        "assembly",
        "index-receipt",
        "publish-admission",
        "verify",
        "unknown",
    ] {
        let args = [phase, "-", "-", "-"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            validate_job_outputs(std::slice::from_ref(&output), &[helper(OciDelivery, args)])
                .is_ok(),
            matches!(phase, "admission" | "assembly" | "index-receipt"),
            "{phase}"
        );
    }
    assert!(
        validate_job_outputs(
            std::slice::from_ref(&output),
            &[helper(OciDelivery, vec!["assembly".to_owned()])]
        )
        .is_err()
    );
    assert!(
        validate_job_outputs(
            std::slice::from_ref(&output),
            &[helper(NativePublishReceiptVerifier, Vec::new())]
        )
        .is_ok()
    );
    output.value.output = ActionOutput::ArtifactDigest;
    assert!(
        validate_job_outputs(
            &[output],
            &[helper(
                OciDelivery,
                vec![
                    "assembly".to_owned(),
                    "-".to_owned(),
                    "-".to_owned(),
                    "-".to_owned()
                ]
            )]
        )
        .is_err()
    );
}

#[test]
fn native_oci_release_and_recovery_outputs_reject_foreign_phases() {
    use crate::workflow::source_helper::SourceBoundOperation::OciDelivery;
    let mut output = output();
    for (name, expected) in [
        (ActionOutput::ReleaseVersion, "verify"),
        (ActionOutput::OciExisting, "admission"),
    ] {
        output.value.output = name;
        for phase in [
            "verify",
            "admission",
            "assembly",
            "index-receipt",
            "publish-admission",
        ] {
            let args = [phase, "-", "-", "-"]
                .into_iter()
                .map(str::to_owned)
                .collect();
            assert_eq!(
                validate_job_outputs(std::slice::from_ref(&output), &[helper(OciDelivery, args)])
                    .is_ok(),
                phase == expected
            );
        }
    }
}
