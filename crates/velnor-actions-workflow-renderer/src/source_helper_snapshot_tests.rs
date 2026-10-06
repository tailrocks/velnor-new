use super::credentials::validate_record;
use super::source_helper_step;
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{
    CompiledNativeExecRecipe, CompiledSourceHelper, HelperInvocation, SourceBoundHelper,
    SourceBoundOperation,
};

const GH_TOKEN: &str = "${{ github.token }}";

fn record(
    scope: NativeCredentialScope,
    environment: BTreeMap<String, String>,
) -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::RustReleaseSourceSnapshot;
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let selectors = vec!["rust@1.98.1".to_owned()];
    let invocation =
        HelperInvocation::compiled(descriptor, Vec::new(), selectors.clone()).expect("invocation");
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        vec![
            "/usr/bin/env".to_owned(),
            "-i".to_owned(),
            "/usr/bin/mise".to_owned(),
            "rust@1.98.1".to_owned(),
            "--".to_owned(),
        ],
        environment.clone(),
        selectors,
        scope,
    )
    .expect("recipe");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("record")
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .expect("recipe binding")
        .with_github_output()
        .expect("snapshot output capability")
}

#[test]
fn read_only_snapshot_output_is_admitted() {
    let environment = BTreeMap::from([(String::from("GH_TOKEN"), String::from(GH_TOKEN))]);
    let helper = record(NativeCredentialScope::GithubReadOnly, environment.clone());
    assert!(helper.github_output());
    assert!(validate_record(&helper).is_ok());
    assert!(source_helper_step("Snapshot source", &helper, environment).is_ok());
}

#[test]
fn anonymous_snapshot_output_is_rejected() {
    let helper = record(NativeCredentialScope::Anonymous, BTreeMap::new());
    assert!(validate_record(&helper).is_err());
    assert!(source_helper_step("Snapshot source", &helper, BTreeMap::new()).is_err());
}

#[test]
fn foreign_snapshot_credential_scope_is_rejected() {
    let helper = record(NativeCredentialScope::OciRegistryPublish, BTreeMap::new());
    assert!(validate_record(&helper).is_err());
    assert!(source_helper_step("Snapshot source", &helper, BTreeMap::new()).is_err());
}
