use super::{
    PREPARE_PINNED_TOOLS_STEP, PinnedTool, StepKind, StepRole, ToolCatalog, WorkflowIr,
    audit_prepare_installs_with_catalog, call_step, ir, step,
};
use crate::workflow::VERIFY_JACTIONLINT_STEP;
use velnor_actions_workflow_renderer::verifier_composite::VERIFY_JACTIONLINT_USES;

/// One plan job installing jactionlint, with or without `Verify jactionlint`.
fn jactionlint_ir(spec: &str, verified: bool) -> WorkflowIr {
    let mut workflow = ir(false);
    let job = workflow.jobs.get_mut("plan").expect("plan job");
    job.steps = vec![step(
        PREPARE_PINNED_TOOLS_STEP,
        Some(StepRole::PreparePinnedTools),
        &["mise", "--no-config", "install", spec],
    )];
    if verified {
        job.steps.push(call_step(
            VERIFY_JACTIONLINT_STEP,
            VERIFY_JACTIONLINT_USES,
            Some(StepRole::VerifyJactionlint),
        ));
    }
    workflow
}

#[test]
fn a_verified_catalogued_jactionlint_needs_no_lock_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let verified = audit_prepare_installs_with_catalog(
        dir.path(),
        &jactionlint_ir(&spec, true),
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    assert!(verified.blocking.is_empty(), "{:?}", verified.blocking);
    assert_eq!(verified.recommendation, None);
    let unverified = audit_prepare_installs_with_catalog(
        dir.path(),
        &jactionlint_ir(&spec, false),
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    let summary = unverified
        .recommendation
        .expect("gap without the verify step");
    assert!(summary.contains(&spec), "{summary}");
}

#[test]
fn a_verified_uncatalogued_jactionlint_still_needs_its_lock_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Jactionlint, "9.9.9")
        .expect("exact");
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &jactionlint_ir("jactionlint@9.9.9", true),
        "ubuntu-26.04",
        &[],
        &catalog,
    );
    let summary = outcome
        .recommendation
        .expect("gap for an uncatalogued version");
    assert!(summary.contains("jactionlint@9.9.9"), "{summary}");
}

#[test]
fn jactionlint_verifier_must_follow_the_pinned_tool_install_immediately() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let mut workflow = jactionlint_ir(&spec, true);
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .insert(1, step("Unrelated step", None, &["echo", "between"]));
    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("jactionlint_verifier_sequence_invalid:plan:")),
        "{:?}",
        outcome.blocking
    );
}

#[test]
fn forged_jactionlint_role_with_an_unknown_action_ref_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let mut workflow = jactionlint_ir(&spec, true);
    let call = &mut workflow.jobs.get_mut("plan").expect("plan job").steps[1];
    let StepKind::Action { uses, .. } = &mut call.kind else {
        panic!("jactionlint verifier call is an action");
    };
    *uses = "./.github/actions/velnor-verify-jactionlint-extra".to_owned();

    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("forged_verifier_call:plan:")),
        "forged typed verifier authority must not authorize a near-match ref: {:?}",
        outcome.blocking
    );
}

#[test]
fn duplicate_jactionlint_verifier_calls_are_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let mut workflow = jactionlint_ir(&spec, true);
    let call = workflow.jobs.get_mut("plan").expect("plan job").steps[1].clone();
    workflow
        .jobs
        .get_mut("plan")
        .expect("plan job")
        .steps
        .push(call);

    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("duplicate_verifier_call:plan:jactionlint")),
        "duplicate Jactionlint checks must not attest to one install: {:?}",
        outcome.blocking
    );
}

#[test]
fn duplicate_jactionlint_calls_across_typed_and_inline_forms_are_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let mut workflow = jactionlint_ir(&spec, true);
    workflow.jobs.get_mut("plan").expect("plan job").steps.push(
        crate::workflow::workflow_policy::jactionlint_verify_step::verifier_shell_step(
            &ToolCatalog::pinned(),
        )
        .expect("production inline Jactionlint verifier"),
    );

    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("duplicate_verifier_call:plan:jactionlint")),
        "typed plus inline Jactionlint calls must count as duplicates: {:?}",
        outcome.blocking
    );
}

#[test]
fn verifier_reference_prefix_is_rejected_without_role_or_name_claims() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = format!("jactionlint@{}", velnor_actions_mise::JACTIONLINT_VERSION);
    let mut workflow = jactionlint_ir(&spec, true);
    let call = &mut workflow.jobs.get_mut("plan").expect("plan job").steps[1];
    call.name = "Unrelated action label".to_owned();
    call.role = None;
    let StepKind::Action { uses, .. } = &mut call.kind else {
        panic!("jactionlint verifier call is an action");
    };
    *uses = "./.github/actions/velnor-verify-jactionlint-near-match".to_owned();

    let outcome = audit_prepare_installs_with_catalog(
        dir.path(),
        &workflow,
        "ubuntu-26.04",
        &[],
        &ToolCatalog::pinned(),
    );
    assert!(
        outcome
            .blocking
            .iter()
            .any(|message| message.starts_with("forged_verifier_call:plan:")),
        "unknown verifier-prefix references must fail even with neutral metadata: {:?}",
        outcome.blocking
    );
}
