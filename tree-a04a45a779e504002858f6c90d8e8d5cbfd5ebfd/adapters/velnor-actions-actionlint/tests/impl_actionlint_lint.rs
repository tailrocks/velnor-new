//! Lint-audit cases: manifest purity, bridge exactness, canonical refs,
//! input schemas, and stable rejection codes.
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_actionlint::actions::{
    ALINT_ACTION_SHA, ALINT_ACTION_VERSION, CHECKOUT_ACTION_SHA, CHECKOUT_ACTION_VERSION,
};
use velnor_actions_actionlint::{
    ALINT_ACTION, ActionlintCapabilities, ActionlintConfigInput, ActionlintError, CHECKOUT_ACTION,
    PinnedActionRef, RUNNER_LABEL_BRIDGE, StepSyntax, checkout_inputs_schema,
    render_actionlint_yaml, validate_action_inputs,
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
fn manifest_declares_contract_family_only() {
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
    assert_eq!(deps.len(), 3, "exactly the contract family expected");
    for dep in &deps {
        assert!(
            dep.starts_with("velnor-actions-contract"),
            "family-only dependency: {dep}"
        );
    }
}

#[test]
fn config_variable_blank_and_whitespace_rejected() {
    for name in ["", " ", "MY VAR", "MY-VAR", "MY.VAR", " TRAILING"] {
        let input = ActionlintConfigInput::new("0.1.0").with_config_variable(name);
        assert!(
            matches!(
                render_actionlint_yaml(&input),
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
            ActionlintError::InvalidWorkflowPath {
                path: "workflows/x.yml".to_owned(),
            },
            "invalid_workflow_path: workflows/x.yml",
        ),
        (
            ActionlintError::ConsumerIgnoreForbidden {
                rule: "shellcheck".to_owned(),
            },
            "consumer_ignore_forbidden: shellcheck",
        ),
        (
            ActionlintError::BroadIgnore {
                path: ".github/workflows/**".to_owned(),
            },
            "broad_ignore: .github/workflows/**",
        ),
        (
            ActionlintError::InvalidIgnore {
                problem: "missing_justification:shellcheck:.github/workflows/v.yml".to_owned(),
            },
            "invalid_ignore: missing_justification:shellcheck:.github/workflows/v.yml",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn bridge_block_carries_configured_label() {
    for label in ["ubuntu-26.04", "ubuntu-24.04", "ubuntu-22.04"] {
        let input = ActionlintConfigInput::new("0.1.0").with_runner_label(label);
        let output = render_actionlint_yaml(&input).expect("renders");
        assert!(
            !output.yaml.contains("runs-on"),
            "config must never carry job runner labels"
        );
        assert!(
            output.yaml.contains(&format!("    - {label}\n")),
            "bridge must emit the configured label {label}:\n{}",
            output.yaml
        );
    }
}

#[test]
fn bridge_block_carries_exact_compat_label() {
    let input = ActionlintConfigInput::new("0.1.0").with_runner_label(RUNNER_LABEL_BRIDGE);
    let output = render_actionlint_yaml(&input).expect("renders");
    assert!(
        !output.yaml.contains("runs-on"),
        "config must never carry job runner labels"
    );
    let mut labels = Vec::new();
    let mut in_labels = false;
    for line in output.yaml.lines() {
        if line == "self-hosted-runner:" {
            in_labels = false;
            continue;
        }
        if line == "  labels:" {
            in_labels = true;
            continue;
        }
        if in_labels {
            if let Some(label) = line.strip_prefix("    - ") {
                labels.push(label.to_owned());
            } else {
                break;
            }
        }
    }
    assert_eq!(labels, vec![RUNNER_LABEL_BRIDGE.to_owned()]);
}

#[test]
fn bridge_label_shape_is_hosted_ubuntu() {
    assert_eq!(RUNNER_LABEL_BRIDGE, "ubuntu-26.04");
    let version = RUNNER_LABEL_BRIDGE
        .strip_prefix("ubuntu-")
        .expect("hosted ubuntu label");
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(parts.len(), 2, "ubuntu-NN.04 shape");
    assert!(
        parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit())),
        "numeric dotted version"
    );
}

#[test]
fn bridge_label_missing_or_unlisted_fails_closed() {
    let missing = render_actionlint_yaml(&ActionlintConfigInput::new("0.1.0"));
    assert!(
        matches!(missing, Err(ActionlintError::InvalidRunnerLabel { .. })),
        "unset label must fail, never emit a hardcoded distro"
    );
    for label in ["ubuntu-latest", "ubuntu-99.04", "self-hosted", ""] {
        let input = ActionlintConfigInput::new("0.1.0").with_runner_label(label);
        assert!(
            matches!(
                render_actionlint_yaml(&input),
                Err(ActionlintError::InvalidRunnerLabel { .. })
            ),
            "unlisted label must fail: {label}"
        );
    }
}

#[test]
fn output_reports_bridge_state() {
    let input = ActionlintConfigInput::new("0.1.0").with_runner_label(RUNNER_LABEL_BRIDGE);
    let default = render_actionlint_yaml(&input).expect("renders");
    assert!(default.runner_bridge_emitted);
    assert!(default.yaml.contains("self-hosted-runner:"));
    let caps = ActionlintCapabilities::for_pinned().recognize_hosted_label_26_04();
    let input = ActionlintConfigInput::new("0.1.0").with_capabilities(caps);
    let recognized = render_actionlint_yaml(&input).expect("renders");
    assert!(!recognized.runner_bridge_emitted);
    assert!(!recognized.yaml.contains("self-hosted-runner:"));
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
    let caps = ActionlintCapabilities::for_pinned();
    assert_eq!(caps.check_step_syntax(StepSyntax::JobMatrix), Ok(()));
    let error = caps
        .check_step_syntax(StepSyntax::NativeParallelism)
        .expect_err("native syntax unqualified");
    assert_eq!(error.to_string(), "unsupported_syntax: native_parallelism");
}
