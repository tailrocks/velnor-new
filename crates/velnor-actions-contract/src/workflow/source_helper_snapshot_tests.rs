use super::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    compiled_source_sha256,
};
use crate::workflow::native_tools::{CompiledNativeExecRecipe, NativeCredentialScope};
use std::collections::BTreeMap;

fn invocation(operation: SourceBoundOperation, args: Vec<String>) -> HelperInvocation {
    let source = crate::generated_source("0.1.0", "exit 0\n").expect("generated source");
    let digest = compiled_source_sha256(source.as_bytes());
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    HelperInvocation::compiled(descriptor, args, vec!["rust@1.98.1".to_owned()])
        .expect("invocation")
}

fn snapshot() -> CompiledSourceHelper {
    let source = crate::generated_source("0.1.0", "exit 0\n").expect("generated source");
    CompiledSourceHelper::compiled(
        invocation(SourceBoundOperation::RustReleaseSourceSnapshot, Vec::new()),
        source,
    )
    .expect("snapshot record")
}

fn recipe(scope: NativeCredentialScope) -> CompiledNativeExecRecipe {
    CompiledNativeExecRecipe::compiled_for_scope(
        vec![
            "/usr/bin/env".to_owned(),
            "-i".to_owned(),
            "/usr/bin/mise".to_owned(),
            "rust@1.98.1".to_owned(),
            "--".to_owned(),
        ],
        BTreeMap::new(),
        vec!["rust@1.98.1".to_owned()],
        scope,
    )
    .expect("recipe")
}

#[test]
fn source_snapshot_requires_exact_zero_arguments() {
    let valid = invocation(SourceBoundOperation::RustReleaseSourceSnapshot, Vec::new());
    assert!(valid.args().is_empty());
    assert!(
        HelperInvocation::compiled(
            valid.descriptor().clone(),
            vec!["unexpected".to_owned()],
            valid.installed_selectors().to_vec(),
        )
        .is_err()
    );
}

#[test]
fn output_capability_getter_and_foreign_operation_are_closed() {
    let capability = snapshot()
        .with_github_output()
        .expect("snapshot output capability");
    assert!(capability.github_output());
    assert!(capability.validate_binding().is_err());

    let foreign = CompiledSourceHelper::compiled(
        invocation(SourceBoundOperation::RustPrepareRootLinux, Vec::new()),
        crate::generated_source("0.1.0", "exit 0\n").expect("generated source"),
    )
    .expect("foreign record");
    assert!(foreign.with_github_output().is_err());
}

#[test]
fn snapshot_output_requires_read_only_recipe() {
    let anonymous = snapshot()
        .with_execution_recipe(recipe(NativeCredentialScope::Anonymous))
        .expect("anonymous recipe")
        .with_github_output()
        .expect("eligible snapshot output");
    assert!(anonymous.validate_binding().is_err());

    let read_only = snapshot()
        .with_execution_recipe(recipe(NativeCredentialScope::GithubReadOnly))
        .expect("read-only recipe")
        .with_github_output()
        .expect("read-only snapshot output");
    assert!(read_only.github_output());
    assert!(read_only.validate_binding().is_ok());
}

#[test]
fn output_environment_path_cannot_be_supplied_by_owner_record() {
    let forged = snapshot().with_environment(BTreeMap::from([(
        "GITHUB_OUTPUT".to_owned(),
        "/forged/output".to_owned(),
    )]));
    assert!(forged.with_github_output().is_err());

    let bound = snapshot()
        .with_execution_recipe(recipe(NativeCredentialScope::GithubReadOnly))
        .expect("read-only recipe")
        .with_github_output()
        .expect("snapshot output");
    let edited = bound.with_environment(BTreeMap::from([(
        "GITHUB_OUTPUT".to_owned(),
        "/edited/output".to_owned(),
    )]));
    assert!(edited.validate_binding().is_err());
}
