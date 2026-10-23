//! Generated verifier-action definitions and calls remain closed and typed.

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepId, StepKind, StepRole};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::render::RenderedFile;

use super::audit_with_verifiers;
use crate::workflow::verifier_actions::{VerifierAction, VerifierActionSet, call_step};

fn verifiers() -> VerifierActionSet {
    VerifierActionSet::build(
        &ToolCatalog::pinned().for_emission(),
        env!("CARGO_PKG_VERSION"),
    )
    .expect("canonical verifier actions")
}

fn action_call(action: &VerifierAction) -> String {
    format!(
        "      - name: {}\n{}",
        velnor_actions_workflow_renderer::yaml::quote_scalar(action.name),
        action_uses_line(action)
    )
}

fn action_uses_line(action: &VerifierAction) -> String {
    action_uses_ref_line(action.uses)
}

fn action_uses_ref_line(uses: &str) -> String {
    format!("        uses: {uses} # zizmor: ignore[self-repository]\n")
}

fn workflow(verifiers: &VerifierActionSet) -> String {
    format!(
        "jobs:\n  verify:\n    steps:\n{}      - name: Install pinned Rust\n        run: mise install rust@1.99.1\n{}{}",
        action_call(&verifiers.restored_rust),
        action_call(&verifiers.installed_rust),
        action_call(&verifiers.jactionlint),
    )
}

fn files(verifiers: &VerifierActionSet, workflow: String) -> Vec<RenderedFile> {
    let mut files = vec![RenderedFile {
        path: ".github/workflows/verify.yml".to_owned(),
        bytes: workflow,
    }];
    files.extend(verifiers.files());
    files
}

fn assert_rejected(files: &[RenderedFile], verifiers: &VerifierActionSet, reason: &str) {
    let error = audit_with_verifiers(files, verifiers)
        .expect_err("mutated verifier input must fail closed")
        .to_string();
    assert!(error.contains(reason), "{error}");
}

fn typed_call(action: &VerifierAction) -> Step {
    call_step("Neutral display label", action.uses, Some(action.role))
}

fn mutate_action(
    step: &mut Step,
    update: impl FnOnce(&mut String, &mut BTreeMap<String, String>, &mut BTreeMap<String, String>),
) {
    let StepKind::Action { uses, with, env } = &mut step.kind else {
        panic!("typed verifier fixture is an action step");
    };
    update(uses, with, env);
}

fn assert_typed_call_rejected(action: &VerifierAction, update: impl FnOnce(&mut Step)) {
    let mut step = typed_call(action);
    update(&mut step);
    assert!(
        !VerifierActionSet::canonical_call(&step, action),
        "typed verifier call must reject caller-controlled authority"
    );
}

#[test]
fn generated_calls_use_role_and_ref_authority_not_display_labels() {
    let verifiers = verifiers();
    let actions = [
        &verifiers.restored_rust,
        &verifiers.installed_rust,
        &verifiers.jactionlint,
    ];
    for action in actions {
        assert!(VerifierActionSet::canonical_call(
            &typed_call(action),
            action
        ));
    }

    let generated = files(&verifiers, workflow(&verifiers));
    assert!(
        audit_with_verifiers(&generated, &verifiers).is_ok(),
        "generated definitions and canonical workflow calls must pass"
    );

    assert_typed_call_rejected(&verifiers.installed_rust, |step| {
        step.role = Some(StepRole::VerifyJactionlint);
    });
    assert_typed_call_rejected(&verifiers.jactionlint, |step| {
        step.role = Some(StepRole::VerifyRustManifest);
    });
    assert_typed_call_rejected(&verifiers.installed_rust, |step| {
        mutate_action(step, |uses, _, _| {
            *uses = verifiers.restored_rust.uses.to_owned();
        });
    });
    assert_typed_call_rejected(&verifiers.installed_rust, |step| {
        mutate_action(step, |_, with, _| {
            with.insert("phase".to_owned(), "restored".to_owned());
        });
    });
    assert_typed_call_rejected(&verifiers.installed_rust, |step| {
        mutate_action(step, |_, _, env| {
            env.insert("RUSTUP_HOME".to_owned(), "/tmp/other".to_owned());
        });
    });
    assert_typed_call_rejected(&verifiers.installed_rust, |step| {
        step.id = Some(StepId::Plan);
    });
    assert_typed_call_rejected(&verifiers.installed_rust, |step| {
        step.condition = Some("always()".to_owned());
    });
    assert_typed_call_rejected(&verifiers.installed_rust, |step| {
        step.parallel = true;
    });
}

#[test]
fn audit_requires_each_generated_action_definition_exactly_once() {
    let verifiers = verifiers();
    let canonical = workflow(&verifiers);

    let mut missing = files(&verifiers, canonical.clone());
    missing.retain(|file| file.path != verifiers.installed_rust.file.path);
    assert_rejected(
        &missing,
        &verifiers,
        "missing or duplicate verifier action definition",
    );

    let mut duplicate = files(&verifiers, canonical.clone());
    duplicate.push(verifiers.installed_rust.file.clone());
    assert_rejected(
        &duplicate,
        &verifiers,
        "missing or duplicate verifier action definition",
    );

    let mut unexpected = files(&verifiers, canonical.clone());
    unexpected.push(RenderedFile {
        path: ".github/actions/velnor-verify-unregistered/action.yml".to_owned(),
        bytes: "runs:\n  using: composite\n  steps: []\n".to_owned(),
    });
    assert_rejected(
        &unexpected,
        &verifiers,
        "unexpected generated verifier definition",
    );

    let mut malformed = files(&verifiers, canonical);
    let definition = malformed
        .iter_mut()
        .find(|file| file.path == verifiers.jactionlint.file.path)
        .expect("generated Jactionlint definition");
    definition.bytes = "runs:\n  using: composite\n  steps: [\n".to_owned();
    assert_rejected(
        &malformed,
        &verifiers,
        "verifier action body, shell, environment, or marker changed",
    );
}

#[test]
fn audit_rejects_jactionlint_metadata_and_rust_phase_substitution() {
    let verifiers = verifiers();
    let canonical = workflow(&verifiers);
    let jactionlint_call = action_call(&verifiers.jactionlint);
    let unregistered_jactionlint_ref = format!("{}-extra", verifiers.jactionlint.uses);
    let wrong_ref = canonical.replace(
        &action_uses_line(&verifiers.jactionlint),
        &action_uses_ref_line(&unregistered_jactionlint_ref),
    );
    let extra_with = canonical.replace(
        &jactionlint_call,
        &format!("{jactionlint_call}        with:\n          phase: installed\n"),
    );
    let extra_env = canonical.replace(
        &jactionlint_call,
        &format!("{jactionlint_call}        env:\n          RUSTUP_HOME: /tmp/other\n"),
    );
    let condition = canonical.replace(
        &jactionlint_call,
        &jactionlint_call.replacen("        uses:", "        if: always()\n        uses:", 1),
    );
    let wrong_rust_phase = canonical.replace(
        &action_uses_line(&verifiers.installed_rust),
        &action_uses_line(&verifiers.restored_rust),
    );

    for changed in [wrong_ref, extra_with, extra_env, condition] {
        assert_ne!(changed, canonical, "each case must mutate a generated call");
        assert_rejected(
            &files(&verifiers, changed),
            &verifiers,
            "noncanonical verifier call or body",
        );
    }
    assert_ne!(wrong_rust_phase, canonical);
    assert_rejected(
        &files(&verifiers, wrong_rust_phase),
        &verifiers,
        "no verification follows the install",
    );
}
