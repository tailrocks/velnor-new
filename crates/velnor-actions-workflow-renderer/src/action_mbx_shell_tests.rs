use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};

use super::{
    ACTION_MBX_BIN, RESOLVE_ACTION_MBX_BIN, candidate_build_argv, mbx_probe_argv,
    role_uses_resolver, validate_argv, validate_argv_for_role, validate_step_argv,
};
use crate::{RenderError, cache_steps};

fn version_check() -> Step {
    cache_steps::mbx_version_check_step("1.23.0", "1.99.0", BTreeMap::new()).expect("version check")
}

fn cleanup() -> Step {
    let action = Step {
        parallel: false,
        name: cache_steps::MBX_RESTORE_NAME.to_owned(),
        id: None,
        role: Some(StepRole::MbxCache),
        condition: None,
        kind: StepKind::Action {
            uses: format!("{}@{}", cache_steps::MBX_ACTION_NAME, "a".repeat(40)),
            with: BTreeMap::from([
                ("version".to_owned(), "1.23.0".to_owned()),
                ("toolchain".to_owned(), "1.99.0".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    };
    cache_steps::mbx_workspace_clean_step_for_action(&action).expect("cleanup")
}

fn build() -> Step {
    crate::preseed::preseed_build_step(&candidate_build_argv("1.99.0", false), &BTreeMap::new())
        .expect("preseed build")
}

fn verify_build() -> Step {
    crate::preseed::preseed_verify_step(&mbx_probe_argv("1.99.0"), "1.23.0", &BTreeMap::new())
        .expect("preseed verification")
}

fn run(step: &Step) -> &[String] {
    let StepKind::Shell { run, .. } = &step.kind else {
        panic!("MBX template must be shell");
    };
    run
}

#[test]
fn whole_templates_accept_all_four_producer_roles() {
    for step in [version_check(), cleanup(), build(), verify_build()] {
        assert!(role_uses_resolver(step.role));
        validate_argv_for_role(run(&step), step.role).expect("canonical MBX template");
        crate::commands::join_validated_argv_for_run(run(&step)).expect("canonical template joins");
    }
}

#[test]
fn plain_step_renderer_uses_role_validation_and_keeps_generic_rejection() {
    let step = version_check();
    crate::steps_plain::plain_step_to_yaml(&step)
        .expect("plain renderer accepts the complete action-owned template");

    let mut forged = step;
    let StepKind::Shell { run, .. } = &mut forged.kind else {
        panic!("MBX template must be shell");
    };
    run[2].push_str("; echo injected");
    assert!(crate::steps_plain::plain_step_to_yaml(&forged).is_err());

    let generic = Step {
        name: "generic shell".to_owned(),
        id: None,
        role: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned(), "-c".to_owned(), "echo $(evil)".to_owned()],
            env: BTreeMap::new(),
        },
        ..version_check()
    };
    assert!(crate::steps_plain::plain_step_to_yaml(&generic).is_err());
}

#[test]
fn forged_role_rejects_suffix_order_reassignment_and_comment_payloads() {
    let step = version_check();
    let mut suffix = run(&step).to_vec();
    suffix[2].push_str("; echo injected");
    assert!(validate_argv_for_role(&suffix, step.role).is_err());

    let mut before_resolver = run(&step).to_vec();
    let resolver_at = before_resolver[2]
        .find(RESOLVE_ACTION_MBX_BIN)
        .expect("resolver in canonical script");
    before_resolver[2].insert_str(resolver_at, "\"$mbx_bin\" --version; ");
    assert!(validate_argv_for_role(&before_resolver, step.role).is_err());

    let mut reassigned = run(&step).to_vec();
    let resolver_end = reassigned[2]
        .find(RESOLVE_ACTION_MBX_BIN)
        .expect("resolver in canonical script")
        + RESOLVE_ACTION_MBX_BIN.len();
    reassigned[2].insert_str(resolver_end, "; mbx_bin=/tmp/evil");
    assert!(validate_argv_for_role(&reassigned, step.role).is_err());

    let mut comment_only = run(&step).to_vec();
    assert!(comment_only[2].contains(" -- \"$mbx_bin\" --version"));
    comment_only[2] = comment_only[2].replace(
        " -- \"$mbx_bin\" --version",
        "; : ' -- \"$mbx_bin\"'; echo injected",
    );
    assert!(comment_only[2].contains("; : ' -- \"$mbx_bin\"'"));
    assert!(validate_argv_for_role(&comment_only, step.role).is_err());
}

#[test]
fn canonical_template_without_an_authorized_role_is_rejected() {
    let step = version_check();
    assert!(validate_argv_for_role(run(&step), None).is_err());
}

#[test]
fn generic_steps_still_reject_command_substitution() {
    let argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("echo $(evil); {ACTION_MBX_BIN} --version"),
    ];
    assert!(matches!(
        validate_step_argv(&argv, None),
        Err(RenderError::BadCommand(message)) if message.starts_with("command_substitution:")
    ));
}

#[test]
fn existing_resolver_fragment_remains_generic_validation_only() {
    let step = version_check();
    assert!(matches!(
        crate::commands::validate_command_argv(run(&step)),
        Err(RenderError::BadCommand(message)) if message.starts_with("command_substitution:")
    ));
    validate_argv(run(&step)).expect("typed construction accepts its resolver");
}
