use super::validate;
use std::collections::BTreeMap;
use velnor_actions_contract::workflow::native_tools::{
    CompiledNativeExecRecipe, NativeCredentialScope,
};
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
};

fn record(
    environment: BTreeMap<String, String>,
    recipe_environment: BTreeMap<String, String>,
    assignment: Option<&str>,
) -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::RustPrepareRootLinux;
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let selectors = vec!["rust@1.98.1".to_owned()];
    let invocation =
        HelperInvocation::compiled(descriptor, Vec::new(), selectors.clone()).expect("invocation");
    let mut prefix = vec!["/usr/bin/env".to_owned(), "-i".to_owned()];
    if let Some(value) = assignment {
        prefix.push(format!("RUSTUP_AUTO_INSTALL={value}"));
    }
    prefix.extend([
        "/usr/bin/mise".to_owned(),
        "rust@1.98.1".to_owned(),
        "--".to_owned(),
    ]);
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        prefix,
        recipe_environment,
        selectors,
        NativeCredentialScope::Anonymous,
    )
    .expect("recipe");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("record")
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .expect("recipe binding")
}

#[test]
fn absent_or_literal_zero_policy_is_admitted() {
    assert!(validate(&record(BTreeMap::new(), BTreeMap::new(), None)).is_ok());
    assert!(
        validate(&record(
            BTreeMap::from([(String::from("RUSTUP_AUTO_INSTALL"), String::from("0"))]),
            BTreeMap::new(),
            None,
        ))
        .is_ok()
    );
    assert!(validate(&record(BTreeMap::new(), BTreeMap::new(), Some("0"))).is_ok());
}

#[test]
fn record_and_recipe_values_must_be_exact_zero() {
    for value in ["1", ""] {
        assert!(
            validate(&record(
                BTreeMap::from([(String::from("RUSTUP_AUTO_INSTALL"), value.to_owned())]),
                BTreeMap::new(),
                None,
            ))
            .is_err()
        );
        assert!(
            validate(&record(
                BTreeMap::new(),
                BTreeMap::from([(String::from("RUSTUP_AUTO_INSTALL"), value.to_owned())]),
                None,
            ))
            .is_err()
        );
    }
}

#[test]
fn prefix_requires_literal_zero_or_bound_zero_symbol() {
    for value in ["1", "", "$OTHER_AUTO_INSTALL"] {
        assert!(validate(&record(BTreeMap::new(), BTreeMap::new(), Some(value))).is_err());
    }
    for value in ["$RUSTUP_AUTO_INSTALL", "${RUSTUP_AUTO_INSTALL}"] {
        assert!(
            validate(&record(
                BTreeMap::from([(String::from("RUSTUP_AUTO_INSTALL"), String::from("0"))]),
                BTreeMap::new(),
                Some(value),
            ))
            .is_ok()
        );
    }
    assert!(
        validate(&record(
            BTreeMap::new(),
            BTreeMap::new(),
            Some("$RUSTUP_AUTO_INSTALL"),
        ))
        .is_err()
    );
}
