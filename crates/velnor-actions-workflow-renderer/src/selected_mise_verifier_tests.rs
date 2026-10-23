use velnor_actions_contract::StepRole;

use super::{script_for_version, test_script, validate_argv, validate_role_argv};
use crate::{commands, steps};

fn argv(script: impl Into<String>) -> Vec<String> {
    vec!["sh".to_owned(), "-c".to_owned(), script.into()]
}

fn assert_invalid(script: String) {
    assert!(validate_argv(&argv(script)).is_err());
}

#[test]
fn exact_binder_constructor_adds_typed_role_and_revalidates_scrubbed_argv() {
    let raw = argv(test_script("2.3.4"));
    validate_argv(&raw).expect("the fixed binder slot is accepted");

    let step = steps::jactionlint_verifier_step(
        "Verify jactionlint",
        raw,
        std::collections::BTreeMap::default(),
    )
    .expect("dedicated verifier constructor");
    assert_eq!(step.role, Some(StepRole::VerifyJactionlint));
    let velnor_actions_contract::StepKind::Shell { run, .. } = step.kind else {
        panic!("shell verifier step");
    };
    validate_role_argv(&run).expect("the canonical unset prefix is accepted");
}

#[test]
fn specialized_validation_rejects_missing_duplicate_and_changed_binders() {
    let valid = test_script("2.3.4");
    assert_invalid(valid.replace(
        velnor_actions_contract::config::selected_mise::BIND_SELECTED_MISE_FUNCTION,
        "",
    ));
    assert_invalid(format!(
        "{valid}; {}",
        velnor_actions_contract::config::selected_mise::BIND_SELECTED_MISE_FUNCTION
    ));
    assert_invalid(valid.replacen(
        "velnor_capture_physical_path",
        "velnor_capture_physical_patH",
        1,
    ));
}

#[test]
fn specialized_validation_requires_the_canonical_producer_and_fixed_slot() {
    let valid = test_script("2.3.4");
    let binder = velnor_actions_contract::config::selected_mise::BIND_SELECTED_MISE_FUNCTION;
    assert_eq!(valid.matches(binder).count(), 1);
    assert_invalid(format!("if false; then {valid}; fi"));
    assert_invalid(format!("case x in x) {valid};; esac"));
    assert_invalid(format!("for marker in x; do {valid}; done"));
    assert_invalid(format!("while false; do {valid}; done"));
    assert_invalid(format!("function nested {{ {valid}; }}"));
    assert_invalid(format!("'{valid}'"));
    assert_invalid(valid.replacen("want=", "want=altered", 1));
    assert_invalid(valid.replacen("PATH=", "PATH=/tmp; # PATH=", 1));
    assert_invalid(script_for_version("${VELNOR_TOOL_RUST}"));
}

#[test]
fn generic_argv_validation_stays_strict_outside_the_binder_slot() {
    assert_invalid(format!("{}\n", test_script("2.3.4")));
    assert_invalid(format!("{}\r", test_script("2.3.4")));
    assert_invalid(format!("{}\0", test_script("2.3.4")));
    assert!(commands::validate_command_argv(&argv("printf $(id)\n")).is_err());
    assert!(commands::validate_command_argv(&argv("printf `id`")).is_err());
    assert!(
        steps::shell_step(
            "ordinary shell",
            argv(test_script("2.3.4")),
            std::collections::BTreeMap::default()
        )
        .is_err()
    );
}

#[test]
fn verifier_role_requires_the_fixed_shape_and_canonical_prelude() {
    let valid = test_script("2.3.4");
    let mut wrong_shell = argv(valid.clone());
    wrong_shell[0] = "bash".to_owned();
    assert!(validate_argv(&wrong_shell).is_err());
    let mut wrong_shape = argv(valid);
    wrong_shape.push("extra".to_owned());
    assert!(validate_argv(&wrong_shape).is_err());
    assert!(validate_role_argv(&argv("true")).is_err());
    assert!(validate_role_argv(&argv(test_script("2.3.4"))).is_err());
}

#[test]
fn literal_and_project_jactionlint_versions_are_the_only_version_atoms() {
    validate_argv(&argv(test_script("2.3.4"))).expect("exact catalog version");
    validate_argv(&argv(test_script("${VELNOR_TOOL_JACTIONLINT}")))
        .expect("exact project-version reference");
    for invalid in [
        "${VELNOR_TOOL_RUST}",
        "${VELNOR_TOOL_JACTIONLINT:-2.3.4}",
        "v2.3.4",
        "2.3.4; false",
    ] {
        assert!(
            validate_argv(&argv(script_for_version(invalid))).is_err(),
            "{invalid}"
        );
    }
}
