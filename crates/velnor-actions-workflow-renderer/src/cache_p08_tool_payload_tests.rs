use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_contract::StepKind;

use crate::{RenderContext, setup::MiseSetup, yaml::Yaml};

use super::{ToolsCacheInputs, ToolsCachePayload};

fn pinned_mise() -> MiseSetup {
    MiseSetup {
        uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn inputs<'a>(
    runs_on: &'a str,
    mise_setup: &'a MiseSetup,
    specs: &'a [String],
    rustup_toolchain: Option<&'a str>,
    components: &'a [String],
) -> ToolsCacheInputs<'a> {
    ToolsCacheInputs {
        runs_on,
        target: "x86_64-unknown-linux-gnu",
        mise_setup,
        tool_specs: specs,
        rustup_toolchain,
        rustup_components: components,
    }
}

fn payload(inputs: ToolsCacheInputs<'_>) -> ToolsCachePayload {
    ToolsCachePayload::new(inputs).expect("typed tools payload")
}

fn action_inputs(
    step: &velnor_actions_contract::Step,
) -> &std::collections::BTreeMap<String, String> {
    match &step.kind {
        StepKind::Action { with, .. } => with,
        kind => panic!("expected action, got {kind:?}"),
    }
}

fn rendered_condition(step: &Step) -> String {
    let context = RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: BTreeMap::new(),
    };
    let Yaml::Map(entries) = crate::document_steps::step_to_yaml(
        "rust-amq",
        step,
        &context,
        &[],
        false,
        &BTreeMap::new(),
        None,
        false,
    )
    .expect("render consumer step") else {
        panic!("a rendered step is a mapping");
    };
    entries
        .into_iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("if", Yaml::Str(condition)) => Some(condition),
            _ => None,
        })
        .expect("rendered save condition")
}

#[test]
fn payload_paths_and_restore_save_inputs_are_identical() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned(), "shellcheck@0.11.0".to_owned()];
    let components = vec!["rustfmt".to_owned(), "clippy".to_owned()];
    let payload = payload(inputs(
        "ubuntu-26.04",
        &setup,
        &specs,
        Some("1.98.1"),
        &components,
    ));
    assert_eq!(
        payload.paths(),
        [
            "~/.local/share/mise",
            "${{ runner.temp }}/velnor/rustup",
            "${{ runner.temp }}/velnor/cargo/.crates.toml",
            "${{ runner.temp }}/velnor/cargo/.crates2.json",
            "${{ runner.temp }}/velnor/cargo/bin",
        ]
    );
    let restore = payload.restore_step().expect("restore");
    let save = payload.save_step().expect("save");
    let restore_with = action_inputs(&restore);
    let save_with = action_inputs(&save);
    assert_eq!(
        restore.condition.as_deref(),
        Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
    );
    let save_condition = crate::cache_p08::save_policy::condition();
    assert_eq!(save.condition.as_deref(), Some(save_condition.as_str()));
    assert_eq!(restore_with.get("path"), save_with.get("path"));
    assert_eq!(restore_with.get("key"), save_with.get("key"));
    assert_eq!(restore_with.get("key"), Some(&payload.key_expression()));
    assert!(
        restore_with
            .get("restore-keys")
            .is_none_or(String::is_empty)
    );
}

#[test]
fn save_step_renders_generic_protected_default_branch_policy() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned()];
    let payload = payload(inputs("ubuntu-26.04", &setup, &specs, Some("1.98.1"), &[]));
    let condition = rendered_condition(&payload.save_step().expect("save step"));

    assert_eq!(
        condition,
        "success() && github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && steps.v2.outputs.enabled == 'true'"
    );
    assert!(!condition.contains("github.repository"));
    assert!(!condition.contains("refs/heads/main"));
    assert!(condition.contains("github.event.repository.default_branch"));
    assert!(condition.contains("github.ref_protected == true"));
}

#[test]
fn static_identity_canonicalizes_inputs_and_binds_runner_selector() {
    let setup = pinned_mise();
    let specs_a = vec!["rust@1.98.1".to_owned(), "shellcheck@0.11.0".to_owned()];
    let specs_b = vec!["shellcheck@0.11.0".to_owned(), "rust@1.98.1".to_owned()];
    let components_a = vec!["clippy".to_owned(), "rustfmt".to_owned()];
    let components_b = vec!["rustfmt".to_owned(), "clippy".to_owned()];
    let first = payload(inputs(
        "ubuntu-26.04",
        &setup,
        &specs_a,
        Some("1.98.1"),
        &components_a,
    ));
    let reordered = payload(inputs(
        "ubuntu-26.04",
        &setup,
        &specs_b,
        Some("1.98.1"),
        &components_b,
    ));
    let scale_set = payload(inputs(
        "scale-set:velnor+ubuntu-26.04-scale-set",
        &setup,
        &specs_a,
        Some("1.98.1"),
        &components_a,
    ));
    assert_eq!(first.static_digest(), reordered.static_digest());
    assert_ne!(first.static_digest(), scale_set.static_digest());
    assert_eq!(first.static_digest().len(), 64);
    assert!(
        first
            .static_digest()
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
}

#[test]
fn runtime_identity_is_required_and_changes_the_final_key() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned()];
    let payload = payload(inputs("ubuntu-26.04", &setup, &specs, Some("1.98.1"), &[]));
    let first_identity = "a".repeat(64);
    let second_identity = "b".repeat(64);
    assert_ne!(
        payload
            .key_for_runtime_identity(&first_identity)
            .expect("first identity"),
        payload
            .key_for_runtime_identity(&second_identity)
            .expect("second identity")
    );
    assert!(payload.key_for_runtime_identity("latest").is_err());
    assert!(
        payload
            .key_for_runtime_identity("A".repeat(64).as_str())
            .is_err()
    );
    assert!(
        payload
            .key_expression()
            .contains("steps.v2.outputs.identity")
    );
}

#[test]
fn payload_rejects_untyped_or_mismatched_tool_inputs() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned()];
    let components = vec!["clippy".to_owned()];
    assert!(ToolsCachePayload::new(inputs("ubuntu-26.04", &setup, &[], None, &[])).is_err());
    assert!(
        ToolsCachePayload::new(inputs(
            "ubuntu-latest",
            &setup,
            &specs,
            Some("1.98.1"),
            &components
        ))
        .is_err()
    );
    assert!(
        ToolsCachePayload::new(inputs(
            "ubuntu-26.04",
            &setup,
            &specs,
            Some("1.98.2"),
            &components
        ))
        .is_err()
    );
    assert!(
        ToolsCachePayload::new(inputs("ubuntu-26.04", &setup, &specs, None, &components)).is_err()
    );
    assert!(
        ToolsCachePayload::new(inputs(
            "ubuntu-26.04",
            &setup,
            &specs,
            Some("1.98.1"),
            &["cargo".to_owned()]
        ))
        .is_err()
    );
    let invalid_setup = MiseSetup {
        version: "latest".to_owned(),
        ..setup.clone()
    };
    assert!(
        ToolsCachePayload::new(inputs(
            "ubuntu-26.04",
            &invalid_setup,
            &specs,
            Some("1.98.1"),
            &components
        ))
        .is_err()
    );
}

#[test]
fn payload_key_is_namespaced_to_static_payload_and_runtime_identity() {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned()];
    let components = vec!["clippy".to_owned()];
    let base = payload(inputs(
        "ubuntu-26.04",
        &setup,
        &specs,
        Some("1.98.1"),
        &components,
    ));
    let changed_action_setup = MiseSetup {
        uses: "jdx/mise-action@0000000000000000000000000000000000000000".to_owned(),
        ..setup.clone()
    };
    let changed_action = ToolsCachePayload::new(inputs(
        "ubuntu-26.04",
        &changed_action_setup,
        &specs,
        Some("1.98.1"),
        &components,
    ))
    .expect("different valid action SHA");
    let changed_binary_setup = MiseSetup {
        sha256: "b".repeat(64),
        ..setup.clone()
    };
    let changed_binary = ToolsCachePayload::new(inputs(
        "ubuntu-26.04",
        &changed_binary_setup,
        &specs,
        Some("1.98.1"),
        &components,
    ))
    .expect("different valid binary SHA");
    assert_ne!(base.static_digest(), changed_action.static_digest());
    assert_ne!(base.static_digest(), changed_binary.static_digest());
    let runtime = "c".repeat(64);
    assert!(
        base.key_for_runtime_identity(&runtime)
            .expect("key")
            .starts_with("mise-tools-v2-")
    );
}
