#[cfg(test)]
mod qualification_tests {
    use super::{QualificationRunnerContext, qualification_dispatch_for_parts};
    use velnor_actions_contract::QualificationPhase;

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

    #[test]
    fn dispatch_inputs_select_typed_phase_and_bind_runner_identity() {
        let payload = serde_json::json!({
            "inputs": {
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
}
