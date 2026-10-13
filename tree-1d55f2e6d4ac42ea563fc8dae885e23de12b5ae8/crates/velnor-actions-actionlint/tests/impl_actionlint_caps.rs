//! actionlint capability-flag cases.
use velnor_actions_actionlint::{
    ACTIONLINT_VERSION, ActionlintCapabilities, ActionlintError, StepSyntax,
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
fn native_parallelism_unqualified_by_default() {
    let caps = ActionlintCapabilities::for_pinned();
    assert!(!caps.native_step_parallelism_qualified());
    assert!(matches!(
        caps.check_step_syntax(StepSyntax::NativeParallelism),
        Err(ActionlintError::UnsupportedSyntax { .. })
    ));
}

#[test]
fn qualifier_enables_native_syntax() {
    let caps = ActionlintCapabilities::for_pinned().qualify_native_step_parallelism();
    assert!(caps.native_step_parallelism_qualified());
    assert_eq!(
        caps.check_step_syntax(StepSyntax::NativeParallelism),
        Ok(())
    );
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
