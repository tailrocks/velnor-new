//! actionlint capability-flag cases.
use velnor_actions_actionlint::{
    ACTIONLINT_VERSION, ActionlintCapabilities, STEP_PARALLELISM_IGNORES, StepSyntax,
};

#[test]
fn pinned_version_is_verified_release() {
    assert_eq!(ACTIONLINT_VERSION, "1.7.12");
}

#[test]
fn pinned_defaults_support_matrix_and_jobs() {
    let caps = ActionlintCapabilities::for_pinned();
    assert!(caps.supports_matrix_strategy());
    assert!(caps.supports_job_parallelism());
    assert_eq!(caps.check_step_syntax(StepSyntax::JobMatrix), Ok(()));
}

#[test]
fn native_parallelism_is_emitted_through_the_ignore_bridge() {
    let caps = ActionlintCapabilities::for_pinned();
    assert!(caps.native_step_parallelism_qualified());
    assert!(caps.requires_step_parallelism_bridge());
    assert_eq!(
        caps.check_step_syntax(StepSyntax::NativeParallelism),
        Ok(())
    );
    let argv = velnor_actions_actionlint::step_parallelism_ignore_args();
    assert_eq!(
        argv,
        vec![
            "-ignore".to_owned(),
            STEP_PARALLELISM_IGNORES[0].to_owned(),
            "-ignore".to_owned(),
            STEP_PARALLELISM_IGNORES[1].to_owned(),
        ]
    );
}

#[test]
fn native_parse_support_removes_the_bridge() {
    let caps = ActionlintCapabilities::for_pinned().parse_native_step_parallelism();
    assert!(caps.native_step_parallelism_qualified());
    assert!(!caps.requires_step_parallelism_bridge());
}

#[test]
fn bridge_ignores_are_anchored_to_the_two_exact_messages() {
    for pattern in STEP_PARALLELISM_IGNORES {
        assert!(pattern.starts_with('^'), "{pattern}");
    }
    assert!(STEP_PARALLELISM_IGNORES[0].contains(r#"unexpected key "background""#));
    assert!(STEP_PARALLELISM_IGNORES[1].ends_with('$'));
}

#[test]
fn default_impl_matches_pinned() {
    assert_eq!(
        ActionlintCapabilities::default(),
        ActionlintCapabilities::for_pinned()
    );
}

#[test]
fn bridge_required_until_label_recognized() {
    let pinned = ActionlintCapabilities::for_pinned();
    assert!(!pinned.recognizes_ubuntu_26_04_hosted_label());
    assert!(pinned.requires_runner_label_bridge());
    let recognized = pinned.recognize_hosted_label_26_04();
    assert!(recognized.recognizes_ubuntu_26_04_hosted_label());
    assert!(!recognized.requires_runner_label_bridge());
}
