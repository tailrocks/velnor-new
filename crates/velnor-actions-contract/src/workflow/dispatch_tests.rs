use super::{DispatchInput, DispatchInputType, WorkflowDispatch};

fn input(kind: DispatchInputType, default: &str) -> DispatchInput {
    DispatchInput {
        name: "version".to_owned(),
        input_type: kind,
        required: false,
        description: Some("Choose release version".to_owned()),
        options: Vec::new(),
        default: Some(default.to_owned()),
    }
}

#[test]
fn native_string_empty_default_and_strict_boolean() {
    assert!(input(DispatchInputType::String, "").validate().is_ok());
    for value in ["true", "false", "", "False", "0", "false\n", "${{ false }}"] {
        assert_eq!(
            input(DispatchInputType::Boolean, value).validate().is_ok(),
            matches!(value, "true" | "false"),
            "{value:?}"
        );
    }
}

#[test]
fn native_choices_are_closed_unique_and_literal() {
    let mut choice = input(DispatchInputType::Choice, "stable");
    choice.options = vec!["stable".to_owned(), "nightly".to_owned()];
    assert!(choice.validate().is_ok());
    for default in ["", "unknown", "stable\n"] {
        choice.default = Some(default.to_owned());
        assert!(choice.validate().is_err());
    }
    choice.default = None;
    for options in [vec![], vec![""], vec!["stable", "stable"], vec!["stable\n"]] {
        choice.options = options.into_iter().map(str::to_owned).collect();
        assert!(choice.validate().is_err());
    }
    choice.options = vec!["stable".to_owned()];
    for kind in [DispatchInputType::String, DispatchInputType::Boolean] {
        choice.input_type = kind;
        assert!(choice.validate().is_err());
    }
}

#[test]
fn native_dispatch_names_descriptions_and_order_are_checked() {
    let valid = input(DispatchInputType::String, "");
    for name in ["", "Bad", "a.b", "a b", "é"] {
        let mut bad = valid.clone();
        bad.name = name.to_owned();
        assert!(bad.validate().is_err());
    }
    for description in ["", "text\nnext", "\t", "é"] {
        let mut bad = valid.clone();
        bad.description = Some(description.to_owned());
        assert!(bad.validate().is_err());
    }
    assert!(
        WorkflowDispatch {
            inputs: vec![valid.clone(), valid.clone()]
        }
        .validate()
        .is_err()
    );
    let mut earlier = valid.clone();
    earlier.name = "aaa".to_owned();
    assert!(
        WorkflowDispatch {
            inputs: vec![valid, earlier]
        }
        .validate()
        .is_err()
    );
}
