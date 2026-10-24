use super::{QualificationRunnerContext, qualification_dispatch_for_parts};
use velnor_actions_contract::{QualificationDispatchMode, QualificationPhase};

fn runner() -> QualificationRunnerContext<'static> {
    QualificationRunnerContext {
        repository: Some("owner/project"),
        git_ref: Some("refs/heads/main"),
        ref_protected: Some("true"),
        workflow_ref: Some("owner/project/.github/workflows/ci.yml@refs/heads/main"),
        workflow_sha: Some("0123456789abcdef0123456789abcdef01234567"),
        source_sha: Some("0123456789abcdef0123456789abcdef01234567"),
        run_id: Some("41"),
        run_attempt: Some("3"),
    }
}

fn wrapper_runner() -> QualificationRunnerContext<'static> {
    QualificationRunnerContext {
        workflow_ref: Some(
            "owner/project/.github/workflows/qualification.yml@refs/heads/sea/integration",
        ),
        git_ref: Some("refs/heads/sea/integration"),
        ref_protected: Some("false"),
        ..runner()
    }
}

#[test]
fn dispatch_inputs_select_typed_phase_and_bind_runner_identity() {
    let payload = serde_json::json!({
        "inputs": {
            "mode": "both",
            "campaign": "campaign-2030",
            "phase": "third",
            "predecessor_run_id": "39",
            "predecessor_run_attempt": "2"
        },
        "ref": "refs/heads/main",
        "repository": {
            "full_name": "owner/project",
            "default_branch": "main"
        }
    });
    let context = qualification_dispatch_for_parts("workflow_dispatch", &payload, runner())
        .expect("dispatch context")
        .expect("qualification");
    assert_eq!(context.phase, QualificationPhase::Third);
    assert_eq!(context.campaign, "campaign-2030");
    assert_eq!(context.run_id, 41);
    assert_eq!(context.run_attempt, 3);
    assert_eq!(
        context.predecessor.as_ref().map(|value| value.run_id),
        Some(39)
    );
}

#[test]
fn verification_only_dispatch_derives_cold_and_warm_from_attempt() {
    let payload = serde_json::json!({
        "inputs": { "mode": "verification-only" },
        "ref": "refs/heads/sea/integration",
        "repository": {
            "full_name": "owner/project",
            "default_branch": "main"
        }
    });
    let cold = qualification_dispatch_for_parts(
        "workflow_dispatch",
        &payload,
        QualificationRunnerContext {
            run_attempt: Some("1"),
            ..wrapper_runner()
        },
    )
    .expect("cold dispatch")
    .expect("qualification");
    let warm = qualification_dispatch_for_parts("workflow_dispatch", &payload, wrapper_runner())
        .expect("warm dispatch")
        .expect("qualification");

    assert_eq!(cold.mode, QualificationDispatchMode::VerificationOnly);
    assert_eq!(cold.phase, QualificationPhase::Cold);
    assert_eq!(cold.predecessor, None);
    assert_eq!(warm.phase, QualificationPhase::Warm);
    assert_eq!(warm.campaign, cold.campaign);
    assert_eq!(warm.predecessor.map(|value| value.run_attempt), Some(1));
    assert_eq!(
        warm.predecessor.map(|value| value.run_id),
        Some(warm.run_id)
    );
}

#[test]
fn verification_only_dispatch_rejects_caller_selected_coordinates() {
    let payload = serde_json::json!({
        "inputs": {
            "mode": "verification-only",
            "campaign": "campaign-2030",
            "phase": "cold"
        },
        "ref": "refs/heads/sea/integration",
        "repository": {
            "full_name": "owner/project",
            "default_branch": "main"
        }
    });
    assert!(
        qualification_dispatch_for_parts("workflow_dispatch", &payload, wrapper_runner()).is_err()
    );
}

#[test]
fn verification_only_dispatch_requires_exact_wrapper_and_mode_value() {
    let payload = serde_json::json!({
        "inputs": { "mode": "verification-only" },
        "ref": "refs/heads/sea/integration",
        "repository": {
            "full_name": "owner/project",
            "default_branch": "main"
        }
    });
    let wrong_workflow = QualificationRunnerContext {
        workflow_ref: Some("owner/project/.github/workflows/ci.yml@refs/heads/sea/integration"),
        ..wrapper_runner()
    };
    assert!(
        qualification_dispatch_for_parts("workflow_dispatch", &payload, wrong_workflow).is_err()
    );

    let unsupported_mode = serde_json::json!({
        "inputs": { "mode": "cache-disabled" },
        "ref": "refs/heads/main",
        "repository": {
            "full_name": "owner/project",
            "default_branch": "main"
        }
    });
    assert!(
        qualification_dispatch_for_parts("workflow_dispatch", &unsupported_mode, runner()).is_err()
    );
}

#[test]
fn qualification_wrapper_requires_verification_only_mode() {
    let payload = serde_json::json!({
        "inputs": {
            "mode": "verification-only",
        },
        "ref": "refs/heads/sea/integration",
        "repository": {
            "full_name": "owner/project",
            "default_branch": "main"
        }
    });
    assert!(
        qualification_dispatch_for_parts("workflow_dispatch", &payload, wrapper_runner())
            .expect("wrapper dispatch")
            .is_some()
    );

    let mut legacy = payload;
    legacy["inputs"]["mode"] = serde_json::json!("both");
    legacy["inputs"]["campaign"] = serde_json::json!("campaign-2030");
    legacy["inputs"]["phase"] = serde_json::json!("cold");
    assert!(
        qualification_dispatch_for_parts("workflow_dispatch", &legacy, wrapper_runner()).is_err()
    );
}

#[test]
fn default_branch_qualification_keeps_legacy_modes() {
    let payload = serde_json::json!({
        "inputs": {
            "mode": "both",
            "campaign": "campaign-2030",
            "phase": "cold"
        },
        "ref": "refs/heads/main",
        "repository": {
            "full_name": "owner/project",
            "default_branch": "main"
        }
    });
    let context = QualificationRunnerContext {
        workflow_ref: Some("owner/project/.github/workflows/qualification.yml@refs/heads/main"),
        ..runner()
    };
    assert!(
        qualification_dispatch_for_parts("workflow_dispatch", &payload, context)
            .expect("default qualification dispatch")
            .is_some()
    );
}

#[test]
fn rejects_unrecognized_phase_and_missing_campaign() {
    for inputs in [
        serde_json::json!({ "campaign": "campaign-2030", "phase": "schedule" }),
        serde_json::json!({ "phase": "cold" }),
    ] {
        let payload = serde_json::json!({
            "inputs": inputs,
            "ref": "refs/heads/main",
            "repository": {
                "full_name": "owner/project",
                "default_branch": "main"
            }
        });
        assert!(
            qualification_dispatch_for_parts(
                "workflow_dispatch",
                &payload,
                QualificationRunnerContext {
                    run_attempt: Some("1"),
                    ..runner()
                },
            )
            .is_err()
        );
    }
}

#[test]
fn rejects_partial_or_nonpositive_predecessor_locators() {
    for inputs in [
        serde_json::json!({
            "campaign": "campaign-2030",
            "phase": "warm",
            "predecessor_run_id": "39"
        }),
        serde_json::json!({
            "campaign": "campaign-2030",
            "phase": "warm",
            "predecessor_run_id": "0",
            "predecessor_run_attempt": "2"
        }),
        serde_json::json!({
            "campaign": "campaign-2030",
            "phase": "warm",
            "predecessor_run_id": "39",
            "predecessor_run_attempt": "0"
        }),
        serde_json::json!({
            "campaign": "campaign-2030",
            "phase": "warm",
            "predecessor_run_id": "39",
            "predecessor_run_attempt": "2; exit 1"
        }),
    ] {
        let payload = serde_json::json!({
            "inputs": inputs,
            "ref": "refs/heads/main",
            "repository": {
                "full_name": "owner/project",
                "default_branch": "main"
            }
        });
        assert!(
            qualification_dispatch_for_parts(
                "workflow_dispatch",
                &payload,
                QualificationRunnerContext {
                    run_attempt: Some("1"),
                    ..runner()
                },
            )
            .is_err()
        );
    }
}
