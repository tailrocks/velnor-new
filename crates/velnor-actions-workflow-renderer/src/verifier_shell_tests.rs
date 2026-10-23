//! Tests for typed verifier rendering and identity authority.

use std::collections::BTreeMap;

use velnor_actions_contract::{StepKind, StepRole};

use crate::yaml::Yaml;

use super::render_typed_step;
use runtime::{assert_emitted_boundary, assert_emitted_boundary_with_extra_env, step_entries};

fn yaml_field<'a>(entries: &'a [(String, Yaml)], key: &str) -> &'a Yaml {
    entries
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn str_field<'a>(entries: &'a [(String, Yaml)], key: &str) -> &'a str {
    match yaml_field(entries, key) {
        Yaml::Str(value) => value,
        other => panic!("{key} is not a string: {other:?}"),
    }
}

#[test]
fn verifier_shell_requires_canonical_role_and_unset_prefix() {
    let unset = crate::toolchain_env::credential_unset_prelude();
    let missing_prefix = velnor_actions_contract::Step {
        parallel: false,
        name: crate::VERIFY_RUST_TOOLCHAIN_STEP.to_owned(),
        id: None,
        role: Some(StepRole::VerifyRustManifest),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
            env: crate::toolchain_env::credential_scrub(),
        },
    };
    let StepKind::Shell { run, env: step_env } = &missing_prefix.kind else {
        panic!("shell step");
    };
    assert!(render_typed_step(&missing_prefix, run, step_env, &BTreeMap::new(), false).is_err());

    let mut wrong_role = crate::steps::shell_step(
        crate::VERIFY_RUST_TOOLCHAIN_STEP,
        vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
        BTreeMap::new(),
    )
    .expect("step");
    wrong_role.role = Some(StepRole::Jactionlint);
    let StepKind::Shell { run, env: step_env } = &wrong_role.kind else {
        panic!("shell step");
    };
    assert!(render_typed_step(&wrong_role, run, step_env, &BTreeMap::new(), false).is_err());

    let mut analyzer_with_verifier_name = crate::steps::shell_step(
        "Verify jactionlint",
        vec!["sh".to_owned(), "-c".to_owned(), format!("{unset} true")],
        BTreeMap::new(),
    )
    .expect("step");
    analyzer_with_verifier_name.role = Some(StepRole::Jactionlint);
    let StepKind::Shell { run, env } = &analyzer_with_verifier_name.kind else {
        panic!("shell step");
    };
    assert!(
        render_typed_step(
            &analyzer_with_verifier_name,
            run,
            env,
            &BTreeMap::new(),
            false
        )
        .is_err(),
        "the ambient-auth analyzer role cannot claim the verifier label"
    );
}

#[test]
fn rust_verifier_role_survives_name_changes_and_composite_rendering() {
    let unset = crate::toolchain_env::credential_unset_prelude();
    let mut step = crate::steps::shell_step(
        "Renamed verifier",
        vec!["sh".to_owned(), "-c".to_owned(), format!("{unset} true")],
        BTreeMap::new(),
    )
    .expect("step");
    step.role = Some(StepRole::VerifyRustManifest);
    let StepKind::Shell { run, env } = &step.kind else {
        panic!("shell step");
    };

    let rendered = render_typed_step(&step, run, env, &BTreeMap::new(), false)
        .expect("valid verifier")
        .expect("role selects verifier boundary");
    assert_eq!(
        str_field(step_entries(&rendered), "name"),
        "Renamed verifier"
    );

    step.name = "Verify jactionlint".to_owned();
    let StepKind::Shell { run, env } = &step.kind else {
        panic!("shell step");
    };
    let rendered = render_typed_step(&step, run, env, &BTreeMap::new(), false)
        .expect("Rust verifier role wins over a colliding display name")
        .expect("typed role selects verifier boundary");
    assert_emitted_boundary(&rendered);

    let mut composite = crate::steps::composite_shell_step(
        "Renamed composite verifier",
        vec!["sh".to_owned(), "-c".to_owned(), format!("{unset} true")],
        BTreeMap::from([("TOOLS_DIGEST".to_owned(), "${{ inputs.digest }}".to_owned())]),
    )
    .expect("composite step");
    composite.role = Some(StepRole::VerifyRustManifest);
    let StepKind::Shell { run, env } = &composite.kind else {
        panic!("composite shell step");
    };
    let rendered = render_typed_step(&composite, run, env, &BTreeMap::new(), true)
        .expect("valid composite verifier")
        .expect("role selects verifier boundary");
    let entries = step_entries(&rendered);
    assert_eq!(str_field(entries, "name"), "Renamed composite verifier");
    let Yaml::Map(env) = yaml_field(entries, "env") else {
        panic!("composite verifier environment");
    };
    assert_eq!(str_field(env, "TOOLS_DIGEST"), "${{ inputs.digest }}");

    assert!(
        crate::steps::composite_shell_step(
            "Invalid composite verifier",
            vec!["sh".to_owned(), "-c".to_owned(), format!("{unset} true")],
            BTreeMap::from([("VALUE".to_owned(), "${{ inputs.value }}".to_owned())]),
        )
        .is_err()
    );
}

#[test]
fn jactionlint_verifier_role_survives_neutral_and_colliding_names() {
    for name in [
        "Renamed jactionlint verifier",
        "Verify Rust channel manifest",
    ] {
        let step = crate::steps::jactionlint_verifier_step(
            name,
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                crate::selected_mise_verifier::test_script("2.3.4"),
            ],
            BTreeMap::new(),
        )
        .expect("step");
        let StepKind::Shell { run, env } = &step.kind else {
            panic!("shell step");
        };
        let rendered = render_typed_step(&step, run, env, &BTreeMap::new(), false)
            .expect("typed jactionlint role is independent of presentation")
            .expect("role selects verifier boundary");
        assert_emitted_boundary(&rendered);
        assert_eq!(str_field(step_entries(&rendered), "name"), name);
    }

    let unset = crate::toolchain_env::credential_unset_prelude();
    let mut env = crate::toolchain_env::credential_scrub();
    env.insert("TOOLS_DIGEST".to_owned(), "${{ inputs.digest }}".to_owned());
    let composite = velnor_actions_contract::Step {
        parallel: false,
        name: "Verify Rust channel manifest".to_owned(),
        id: None,
        role: Some(StepRole::VerifyJactionlint),
        condition: None,
        kind: StepKind::Shell {
            run: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                format!(
                    "{unset} {}",
                    crate::selected_mise_verifier::test_script("2.3.4")
                ),
            ],
            env,
        },
    };
    let StepKind::Shell { run, env } = &composite.kind else {
        panic!("composite verifier shell");
    };
    let rendered = render_typed_step(&composite, run, env, &BTreeMap::new(), true)
        .expect("the typed verifier supports composite inputs")
        .expect("role selects the Jactionlint boundary");
    assert_emitted_boundary_with_extra_env(&rendered, &[("TOOLS_DIGEST", "${{ inputs.digest }}")]);
    let Yaml::Map(env) = yaml_field(step_entries(&rendered), "env") else {
        panic!("composite verifier environment");
    };
    assert_eq!(str_field(env, "TOOLS_DIGEST"), "${{ inputs.digest }}");
}

#[path = "verifier_shell_runtime_tests.rs"]
pub(crate) mod runtime;
