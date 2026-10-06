use super::check_step_tokens;
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;
use velnor_actions_contract::{
    CompiledNativeExecRecipe, CompiledSourceHelper, HelperInvocation, SourceBoundHelper,
    SourceBoundOperation, Step, StepKind,
};

const GH_TOKEN: &str = "${{ github.token }}";

fn github_record() -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::NativePagesAdmission;
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let selectors = vec!["python@3.14.0".to_owned()];
    let invocation =
        HelperInvocation::compiled(descriptor, Vec::new(), selectors.clone()).expect("invocation");
    let environment = BTreeMap::from([(String::from("GH_TOKEN"), String::from(GH_TOKEN))]);
    let mut recipe_environment = environment.clone();
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        [
            vec!["/usr/bin/env".to_owned(), "-i".to_owned()],
            recipe_environment
                .iter()
                .map(|(key, _value)| format!("{key}=${{{key}}}"))
                .collect(),
            vec![
                "/owned/mise".to_owned(),
                "--no-config".to_owned(),
                "--no-env".to_owned(),
                "--no-hooks".to_owned(),
                "exec".to_owned(),
            ],
            selectors,
            vec!["--".to_owned()],
        ]
        .concat(),
        std::mem::take(&mut recipe_environment),
        vec!["python@3.14.0".to_owned()],
        NativeCredentialScope::GithubReadOnly,
    )
    .expect("recipe");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("compiled source")
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .expect("bound recipe")
}

fn helper_step(invocation: HelperInvocation, env: BTreeMap<String, String>) -> Step {
    Step {
        id: None,
        name: "Trusted helper".to_owned(),
        condition: None,
        kind: StepKind::SourceBoundHelper { invocation, env },
    }
}

#[test]
fn token_gate_admits_exact_trusted_helper() {
    let record = github_record();
    let step = helper_step(record.invocation().clone(), record.environment().clone());
    assert!(check_step_tokens("plan", &step, &[record]).is_ok());
}

#[test]
fn token_gate_rejects_helper_without_compiled_record() {
    let record = github_record();
    let step = helper_step(record.invocation().clone(), record.environment().clone());
    assert!(check_step_tokens("plan", &step, &[]).is_err());
}

#[test]
fn token_gate_rejects_forged_invocation() {
    let record = github_record();
    let mut wire = serde_json::to_value(record.invocation()).expect("serialize invocation");
    wire["args"] = serde_json::json!(["forged-argument"]);
    let invocation: HelperInvocation = serde_json::from_value(wire).expect("invocation shape");
    let step = helper_step(invocation, record.environment().clone());
    assert!(check_step_tokens("plan", &step, &[record]).is_err());
}

#[test]
fn token_gate_rejects_forged_environment() {
    let record = github_record();
    let environment = BTreeMap::from([(
        String::from("GH_TOKEN"),
        String::from("${{ secrets.FORGED }}"),
    )]);
    let step = helper_step(record.invocation().clone(), environment);
    assert!(check_step_tokens("plan", &step, &[record]).is_err());
}
