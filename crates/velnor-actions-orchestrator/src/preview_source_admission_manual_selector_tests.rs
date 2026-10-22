use super::*;

#[test]
fn workflow_dispatch_rejects_missing_or_malformed_rest_run_fields() {
    let route = "repos/example/app/actions/runs/101/attempts/2";
    for run in [Value::Null, json!({})] {
        let (api, fixture) = GitHubApi::with_fixture("example/app", vec![(route.to_owned(), run)]);
        assert!(source_from_workflow_dispatch(&api, 9, 101, 2).is_err());
        fixture.assert_drained();
    }
}

#[test]
fn selector_manual_run_authentication_failure_never_emits_the_checkout_gate_output() {
    let mut run = manual_run();
    run["id"] = json!(102);
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                "repos/example/app".to_owned(),
                json!({"id":9,"full_name":"example/app"}),
            ),
            (
                "repos/example/app/actions/workflows/ci.yml".to_owned(),
                json!({
                    "id":7,
                    "name":WORKFLOW_DISPLAY_NAME,
                    "path":CI_WORKFLOW_PATH,
                    "state":"active"
                }),
            ),
            (
                "repos/example/app/actions/runs/101/attempts/2".to_owned(),
                run,
            ),
        ],
    );
    let mut output = String::from("preserve-existing-output\n");
    let mut checkout_gate_reached = false;
    let result = select_and_emit_source(
        &api,
        SelectionRequest {
            event: &Value::Null,
            seed: &policy(),
            repository_id: 9,
            authority: &"b".repeat(40),
            current_run: 202,
            current_attempt: 1,
            event_name: "workflow_dispatch",
            job_name: "preview-eligibility",
            manual_run_identity: Some((101, 2)),
            deadline: velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(30))
                .expect("selector deadline"),
        },
        |value| {
            output.push_str(value);
            checkout_gate_reached = true;
            Ok(())
        },
    );
    assert!(format!("{result:?}").contains("preview_source_manual_run_identity"));
    assert_eq!(output, "preserve-existing-output\n");
    assert!(!checkout_gate_reached);
    fixture.assert_drained();
    assert_eq!(
        fixture.observed(),
        [
            "repos/example/app",
            "repos/example/app/actions/workflows/ci.yml",
            "repos/example/app/actions/runs/101/attempts/2"
        ]
    );
}
