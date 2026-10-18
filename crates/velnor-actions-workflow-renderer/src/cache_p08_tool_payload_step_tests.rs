use super::{action_inputs, inputs, payload, pinned_mise, rendered_condition, sample_payload};
use velnor_actions_contract::StepKind;

#[test]
fn payload_paths_and_restore_save_inputs_are_identical() {
    let payload = sample_payload();
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
    let StepKind::Action { uses, env, .. } = &restore.kind else {
        panic!("tools restore wrapper is an action");
    };
    assert_eq!(uses, crate::cache_steps::TOOLS_RESTORE_USES);
    assert!(env.is_empty());
    assert_eq!(
        restore.id,
        Some(velnor_actions_contract::StepId::ToolsCacheRestore)
    );
    assert_eq!(
        crate::cache_steps::validate_tools_restore_call(&restore).expect("registered wrapper call"),
        payload.key_expression()
    );
    assert_eq!(restore_with.get("key"), save_with.get("key"));
    assert_eq!(restore_with.get("key"), Some(&payload.key_expression()));
    assert_eq!(
        restore_with.get(crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT),
        Some(&crate::cache_steps::TOOLS_SEED_ADMITTED_EXPRESSION.to_owned())
    );
    assert_eq!(save_with.get("path"), Some(&payload.paths().join("\n")));
}

#[test]
fn restore_wrapper_admission_rejects_mutated_calls() {
    let restore = sample_payload().restore_step().expect("restore");
    velnor_actions_contract::workflow::step_identity::validate_step_sequence(
        std::slice::from_ref(&restore),
        "tools-cache-test",
    )
    .expect("fixed wrapper is the tools restore authority");
    let mut wrong_wrapper = restore.clone();
    let StepKind::Action { uses, .. } = &mut wrong_wrapper.kind else {
        panic!("tools restore wrapper is an action");
    };
    *uses = "./.github/actions/unregistered-tools-restore".to_owned();
    assert!(
        velnor_actions_contract::workflow::step_identity::validate_step_sequence(
            &[wrong_wrapper],
            "tools-cache-test",
        )
        .is_err()
    );
    let mut wrong_key = restore.clone();
    let StepKind::Action { with, .. } = &mut wrong_key.kind else {
        panic!("tools restore wrapper is an action");
    };
    with.insert("key".to_owned(), "caller-key".to_owned());
    assert!(crate::cache_steps::validate_tools_restore_call(&wrong_key).is_err());
    let mut extra_input = restore.clone();
    let StepKind::Action { with, .. } = &mut extra_input.kind else {
        panic!("tools restore wrapper is an action");
    };
    with.insert("path".to_owned(), "caller-path".to_owned());
    assert!(crate::cache_steps::validate_tools_restore_call(&extra_input).is_err());
    let mut conditional = restore.clone();
    conditional.condition = Some("always()".to_owned());
    assert!(crate::cache_steps::validate_tools_restore_call(&conditional).is_err());
    let mut untyped = restore.clone();
    untyped.role = None;
    assert!(crate::cache_steps::validate_tools_restore_call(&untyped).is_err());
    let mut missing_id = restore.clone();
    missing_id.id = None;
    assert!(crate::cache_steps::validate_tools_restore_call(&missing_id).is_err());
    assert_eq!(
        restore.condition.as_deref(),
        Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
    );
}

#[test]
fn restore_composite_binds_marker_pin_key_and_paths() {
    let payload = sample_payload();
    let save = payload.save_step().expect("save");
    let save_condition = crate::cache_p08::save_policy::condition(
        velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
    );
    assert_eq!(save.condition.as_deref(), Some(save_condition.as_str()));
    let restore_action = crate::cache_steps::tools_restore_action_file("0.1.0")
        .expect("generated tools restore action");
    let bytes = &restore_action.bytes;
    let marker = crate::marker::marker_for_version("0.1.0").expect("generator marker");
    assert!(bytes.starts_with(&format!("{marker}\n")));
    assert_eq!(
        restore_action.path,
        ".github/actions/velnor-tools-cache-restore/action.yml"
    );
    assert!(bytes.contains(&format!(
        "uses: {}",
        crate::cache_steps::TOOLS_RESTORE_ACTION_USES
    )));
    assert!(bytes.contains("key: ${{ inputs.key }}"));
    for path in payload.paths() {
        assert!(bytes.contains(path), "missing {path}");
    }
    assert!(bytes.contains("restore-keys: \"\""));
    assert!(bytes.contains("outputs.cache-hit"));
    assert!(bytes.contains("outputs.cache-matched-key"));
    assert!(bytes.contains("value: ${{ steps.restore.outputs.cache-hit }}"));
    assert!(bytes.contains("value: ${{ steps.restore.outputs.cache-matched-key }}"));
    assert!(bytes.contains("rm -rf"));
    crate::cache_steps::assert_rendered_admission_parses(bytes);
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
