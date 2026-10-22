//! Lint-audit cases: manifest purity, canonical refs, input schemas, and
//! stable rejection codes.
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_actionlint::actions::{
    ALINT_ACTION_SHA, ALINT_ACTION_VERSION, CHECKOUT_ACTION_SHA, CHECKOUT_ACTION_VERSION,
};
use velnor_actions_actionlint::{
    ALINT_ACTION, ActionlintError, CHECKOUT_ACTION, JactionlintCapabilities,
    JactionlintConfigInput, PinnedActionRef, StepSyntax, checkout_inputs_schema,
    render_jactionlint_yaml, validate_action_inputs,
};

/// Dependencies that would let config rendering spawn or fetch.
const FORBIDDEN_MANIFEST_TOKENS: &[&str] = &[
    "tokio",
    "reqwest",
    "ureq",
    "hyper",
    "curl",
    "duct",
    "subprocess",
    "shell-words",
];

#[test]
fn manifest_declares_contract_only() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert!(
        !root.join("build.rs").exists(),
        "no build script may fetch or probe"
    );
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("manifest readable");
    for token in FORBIDDEN_MANIFEST_TOKENS {
        assert!(
            !manifest.contains(token),
            "forbidden manifest token: {token}"
        );
    }
    assert!(!manifest.contains("build ="), "no build key allowed");
    let mut in_deps = false;
    let mut deps = Vec::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps = trimmed == "[dependencies]";
            continue;
        }
        if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') {
            deps.push(trimmed.to_owned());
        }
    }
    assert_eq!(deps.len(), 1, "exactly one dependency expected");
    assert!(
        deps[0].starts_with("velnor-actions-contract"),
        "sole dependency must be the contract: {}",
        deps[0]
    );
}

#[test]
fn config_variable_blank_and_whitespace_rejected() {
    for name in ["", " ", "MY VAR", "MY-VAR", "MY.VAR", " TRAILING"] {
        let input = JactionlintConfigInput::new("0.1.0").with_config_variable(name);
        assert!(
            matches!(
                render_jactionlint_yaml(&input),
                Err(ActionlintError::InvalidConfigVariable { .. })
            ),
            "variable must be rejected: {name:?}"
        );
    }
}

#[test]
fn config_rejection_codes_are_stable() {
    let cases = [
        (
            ActionlintError::InvalidConfigVariable {
                name: "9LIVES".to_owned(),
            },
            "invalid_config_variable: 9LIVES",
        ),
        (
            ActionlintError::InvalidRunnerLabel {
                label: "Bad Label".to_owned(),
            },
            "invalid_runner_label: Bad Label",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn checkout_ref_is_canonical_pin() {
    let reference = PinnedActionRef::checkout();
    assert_eq!(reference.validate(), Ok(()));
    assert_eq!(reference.uses_key(), CHECKOUT_ACTION);
    assert_eq!(reference.sha, CHECKOUT_ACTION_SHA);
    assert_eq!(reference.version_comment, CHECKOUT_ACTION_VERSION);
    let value = format!("{CHECKOUT_ACTION}@{CHECKOUT_ACTION_SHA}");
    assert_eq!(reference.uses_value(), value);
    assert_eq!(
        reference.render_uses(),
        format!("uses: {value} # {CHECKOUT_ACTION_VERSION}")
    );
    assert_eq!(
        PinnedActionRef::parse_uses(&value, CHECKOUT_ACTION_VERSION),
        Ok(reference)
    );
}

#[test]
fn alint_ref_is_canonical_full_sha_pin() {
    let reference = PinnedActionRef::alint();
    assert_eq!(reference.validate(), Ok(()));
    assert_eq!(reference.uses_key(), ALINT_ACTION);
    assert_eq!(reference.sha, ALINT_ACTION_SHA);
    assert_eq!(reference.version_comment, ALINT_ACTION_VERSION);
    let value = format!("{ALINT_ACTION}@{ALINT_ACTION_SHA}");
    assert_eq!(reference.uses_value(), value);
    assert_eq!(
        reference.render_uses(),
        format!("uses: {value} # {ALINT_ACTION_VERSION}")
    );
    assert_eq!(
        PinnedActionRef::parse_uses(&value, ALINT_ACTION_VERSION),
        Ok(reference)
    );
}

#[test]
fn checkout_schema_matches_lint_job_inputs() {
    let schema = checkout_inputs_schema();
    assert_eq!(schema.action, CHECKOUT_ACTION);
    assert_eq!(schema.required, vec!["persist-credentials".to_owned()]);
    assert_eq!(
        schema.allowed_inputs(),
        BTreeSet::from([
            "persist-credentials".to_owned(),
            "ref".to_owned(),
            "fetch-depth".to_owned(),
            "lfs".to_owned(),
        ])
    );
    let inputs = BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]);
    assert_eq!(validate_action_inputs(&schema, &inputs), Ok(()));
}

#[test]
fn checkout_schema_rejects_empty_value() {
    let inputs = BTreeMap::from([("persist-credentials".to_owned(), String::new())]);
    let error = validate_action_inputs(&checkout_inputs_schema(), &inputs);
    assert!(matches!(
        error,
        Err(ActionlintError::InvalidActionInput { .. })
    ));
    assert_eq!(
        error.expect_err("empty value rejected").to_string(),
        "invalid_action_input: actions/checkout: persist-credentials: empty_value"
    );
}

#[test]
fn action_input_codes_are_stable() {
    let unknown = ActionlintError::UnknownActionInput {
        action: CHECKOUT_ACTION.to_owned(),
        input: "injected".to_owned(),
    };
    assert_eq!(
        unknown.to_string(),
        "unknown_action_input: actions/checkout: injected"
    );
    let missing = ActionlintError::MissingActionInput {
        action: CHECKOUT_ACTION.to_owned(),
        input: "persist-credentials".to_owned(),
    };
    assert_eq!(
        missing.to_string(),
        "missing_action_input: actions/checkout: persist-credentials"
    );
    let inputs = BTreeMap::from([
        ("persist-credentials".to_owned(), "false".to_owned()),
        ("injected".to_owned(), "true".to_owned()),
    ]);
    assert_eq!(
        validate_action_inputs(&checkout_inputs_schema(), &inputs),
        Err(unknown)
    );
}

#[test]
fn capability_gate_codes_are_stable() {
    let caps = JactionlintCapabilities::for_pinned();
    for syntax in [
        StepSyntax::JobMatrix,
        StepSyntax::NativeParallelism,
        StepSyntax::CacheMode,
    ] {
        assert_eq!(caps.check_step_syntax(syntax), Ok(()), "{syntax:?}");
    }
    assert_eq!(
        ActionlintError::UnsupportedSyntax {
            syntax: "cache_mode"
        }
        .to_string(),
        "unsupported_syntax: cache_mode"
    );
}
