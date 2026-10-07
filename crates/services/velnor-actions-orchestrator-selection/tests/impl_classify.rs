//! Comparison classification and checkout verification.

use velnor_actions_contract_workflow::WorkflowEvent;
use velnor_actions_mise::CheckDeadline;
use velnor_actions_orchestrator_discovery::discover::Discovery;
use velnor_actions_orchestrator_selection::select::{
    classify_changed, verify_checkout, verify_checkout_until,
};

/// Empty discovery with the non-UTF-8 flag set as given.
fn discovery_broad(skipped_non_utf8: bool) -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: velnor_actions_orchestrator_core::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

#[test]
fn non_utf8_comparison_is_unknown() {
    let discovery = discovery_broad(true);
    let mut warnings = Vec::new();
    let changed = classify_changed(
        std::path::Path::new("/nonexistent"),
        WorkflowEvent::Push,
        Some("base"),
        "head",
        &discovery,
        &mut warnings,
    );
    assert!(changed.is_none());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].starts_with("comparison_unavailable:"));
}

#[test]
fn push_without_base_is_unknown() {
    let discovery = discovery_broad(false);
    let mut warnings = Vec::new();
    let changed = classify_changed(
        std::path::Path::new("/nonexistent"),
        WorkflowEvent::Push,
        None,
        "head",
        &discovery,
        &mut warnings,
    );
    assert!(changed.is_none());
    assert!(
        warnings.iter().any(|w| w.contains("missing_base")),
        "{warnings:?}"
    );
}

#[test]
fn local_without_checkout_is_unknown() {
    let dir = tempfile::tempdir().expect("tempdir");
    let discovery = discovery_broad(false);
    let mut warnings = Vec::new();
    let changed = classify_changed(
        dir.path(),
        WorkflowEvent::Local,
        None,
        "head",
        &discovery,
        &mut warnings,
    );
    assert!(changed.is_none());
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("comparison_unavailable:")),
        "{warnings:?}"
    );
}

#[test]
fn local_checkout_always_verifies() {
    verify_checkout(
        std::path::Path::new("/nonexistent"),
        WorkflowEvent::Local,
        "head",
    )
    .expect("local verifies");
}

#[test]
fn invalid_head_fails_before_git() {
    let err = verify_checkout(
        std::path::Path::new("/nonexistent"),
        WorkflowEvent::Push,
        "evil;rev",
    )
    .expect_err("bad head must fail");
    assert!(err.to_string().contains("bad_head"), "{err}");
}

#[test]
fn missing_checkout_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let err = verify_checkout_until(
        dir.path(),
        WorkflowEvent::Push,
        &"a".repeat(40),
        CheckDeadline::after(std::time::Duration::from_secs(30)).expect("deadline"),
    )
    .expect_err("missing checkout must fail");
    assert!(err.to_string().contains("bad_checkout"), "{err}");
}
