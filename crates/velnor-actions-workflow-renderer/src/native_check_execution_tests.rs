use super::*;
use velnor_actions_contract::config::NativeHomebrewOperation;

fn native_step(operation: NativeHomebrewOperation) -> Step {
    Step {
        name: "Execute native Homebrew check".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::NativeCheckExecution {
            check_id: "formula-contract".to_owned(),
            operation,
            task_id: "internal/native-check/formula-contract".to_owned(),
            sdk_version: env!("CARGO_PKG_VERSION").to_owned(),
        },
    }
}

fn field<'a>(yaml: &'a Yaml, key: &str) -> Option<&'a Yaml> {
    let Yaml::Map(entries) = yaml else {
        return None;
    };
    entries
        .iter()
        .find_map(|(candidate, value)| (candidate == key).then_some(value))
}

#[test]
fn github_read_token_is_scoped_to_each_fixed_native_dispatch() {
    for operation in [
        NativeHomebrewOperation::TapTree,
        NativeHomebrewOperation::FormulaContract,
        NativeHomebrewOperation::Install,
        NativeHomebrewOperation::AdoptionContract,
    ] {
        let rendered =
            render(&native_step(operation), &BTreeMap::new()).expect("native SDK step renders");
        assert_eq!(
            field(&rendered, "env").and_then(|env| field(env, "GH_TOKEN")),
            Some(&Yaml::str("${{ github.token }}".to_owned())),
            "operation {operation:?} must receive the read-only API token"
        );
        let run = field(&rendered, "run").expect("fixed shell dispatch");
        assert!(
            matches!(run, Yaml::Str(value) if value.contains("velnor-actions-")),
            "operation {operation:?} must invoke the staged SDK"
        );
    }
}

#[test]
fn sdk_token_dispatch_rejects_a_different_staged_binary_version() {
    let mut step = native_step(NativeHomebrewOperation::TapTree);
    let StepKind::NativeCheckExecution { sdk_version, .. } = &mut step.kind else {
        panic!("native execution step");
    };
    *sdk_version = "0.0.0".to_owned();
    let error = render(&step, &BTreeMap::new()).expect_err("unmatched SDK cannot receive token");
    assert!(
        error
            .to_string()
            .contains("native_check_sdk_version_mismatch")
    );
}

#[test]
fn generic_shell_environment_still_rejects_github_token_expression() {
    let env = BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]);
    let error = crate::commands::validate_env(&env)
        .expect_err("generic environment expression allowlist remains closed");
    assert!(error.to_string().contains("bad_env_expression:GH_TOKEN"));
}

#[test]
fn native_sdk_token_rejects_job_level_inheritance() {
    let job_env = BTreeMap::from([("GH_TOKEN".to_owned(), "inherited".to_owned())]);
    let error = render(&native_step(NativeHomebrewOperation::TapTree), &job_env)
        .expect_err("native token must be step scoped");
    assert!(
        error
            .to_string()
            .contains("native_check_job_token_inheritance_rejected")
    );
}
