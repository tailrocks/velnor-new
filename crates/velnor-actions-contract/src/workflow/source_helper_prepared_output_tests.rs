use super::{CompiledSourceHelper, HelperInvocation, NativeCredentialScope, SourceBoundOperation};
use crate::workflow::{JobOutput, StepOutputRef};
use crate::{ActionOutput, CompiledNativeExecRecipe, Step, StepId, StepKind};
use std::collections::BTreeMap;

fn invocation(operation: SourceBoundOperation, args: Vec<String>) -> HelperInvocation {
    let source = crate::generated_source("0.1.0", "exit 0\n").expect("source");
    let digest = crate::compiled_source_sha256(source.as_bytes());
    let descriptor = crate::SourceBoundHelper::compiled(operation, operation.path(), &digest)
        .expect("descriptor");
    HelperInvocation::compiled(descriptor, args, vec!["python@3.14.0".to_owned()])
        .expect("invocation")
}

fn record(operation: SourceBoundOperation, scope: NativeCredentialScope) -> CompiledSourceHelper {
    let prefix = [
        "/usr/bin/env",
        "-i",
        "/owned/mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "python@3.14.0",
        "--",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        prefix,
        BTreeMap::new(),
        vec!["python@3.14.0".to_owned()],
        scope,
    )
    .expect("recipe");
    CompiledSourceHelper::compiled(
        invocation(operation, Vec::new()),
        crate::generated_source("0.1.0", "exit 0\n").expect("source"),
    )
    .expect("record")
    .with_execution_recipe(recipe)
    .expect("recipe binding")
}

#[test]
fn prepared_output_channel_requires_anonymous_producer_and_no_path_override() {
    let operation = SourceBoundOperation::RustReleasePreparedPackage;
    let exact = record(operation, NativeCredentialScope::Anonymous)
        .with_github_output()
        .expect("output capability");
    assert!(exact.validate_binding().is_ok());
    let privileged = record(operation, NativeCredentialScope::GithubReadOnly)
        .with_github_output()
        .expect("structural capability");
    assert!(privileged.validate_binding().is_err());
    let override_path = record(operation, NativeCredentialScope::Anonymous).with_environment(
        BTreeMap::from([("GITHUB_OUTPUT".to_owned(), "/tmp/out".to_owned())]),
    );
    assert!(override_path.with_github_output().is_err());
    let verifier = record(
        SourceBoundOperation::RustReleasePackageVerify,
        NativeCredentialScope::Anonymous,
    );
    assert!(verifier.with_github_output().is_err());
}

#[test]
fn prepared_and_verification_invocations_have_no_argument_modes() {
    for operation in [
        SourceBoundOperation::RustReleasePreparedPackage,
        SourceBoundOperation::RustReleasePackageVerify,
    ] {
        let descriptor = invocation(operation, Vec::new()).descriptor().clone();
        assert!(
            HelperInvocation::compiled(
                descriptor,
                vec!["mode".to_owned()],
                vec!["python@3.14.0".to_owned()]
            )
            .is_err()
        );
    }
}

#[test]
fn prepared_blob_output_belongs_only_to_prepared_producer() {
    let output = JobOutput {
        name: "package-blob-sha256".to_owned(),
        value: StepOutputRef {
            step_id: StepId::new("prepared").expect("id"),
            output: ActionOutput::PreparedBlobSha256,
        },
    };
    assert_eq!(
        output.value.expression(),
        "${{ steps.prepared.outputs.package-blob-sha256 }}"
    );
    for (operation, accepted) in [
        (SourceBoundOperation::RustReleasePreparedPackage, true),
        (SourceBoundOperation::RustReleasePackageVerify, false),
        (SourceBoundOperation::RustReleaseSourceSnapshot, false),
        (SourceBoundOperation::RustReleaseAnonymousPackage, false),
    ] {
        let step = Step {
            id: Some(StepId::new("prepared").expect("id")),
            name: "Prepared".to_owned(),
            condition: None,
            kind: StepKind::SourceBoundHelper {
                invocation: invocation(operation, Vec::new()),
                env: BTreeMap::new(),
            },
        };
        assert_eq!(
            crate::workflow::outputs::validate_job_outputs(std::slice::from_ref(&output), &[step])
                .is_ok(),
            accepted
        );
    }
}
