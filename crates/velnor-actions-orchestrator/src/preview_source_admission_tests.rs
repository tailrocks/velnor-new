use super::*;
use serde_json::{Value, json};

#[path = "preview_source_admission_manual_selector_tests.rs"]
mod manual_selector;

fn workflow() -> WorkflowMetadata {
    WorkflowMetadata {
        id: 7,
        name: WORKFLOW_DISPLAY_NAME.to_owned(),
        path: CI_WORKFLOW_PATH.to_owned(),
        state: "active".to_owned(),
    }
}

fn event() -> Value {
    json!({
        "action": "completed",
        "repository": {"id": 9, "full_name": "jackin-project/jackin"},
        "workflow": {
            "id": 7,
            "name": WORKFLOW_DISPLAY_NAME,
            "path": CI_WORKFLOW_PATH
        },
        "workflow_run": {
            "id": 101,
            "run_attempt": 2,
            "workflow_id": 7,
            "head_sha": "a".repeat(40),
            "head_branch": "main",
            "event": "push",
            "status": "completed",
            "conclusion": "success",
            "path": CI_WORKFLOW_PATH,
            "name": WORKFLOW_DISPLAY_NAME,
            "repository": {"id": 9, "full_name": "jackin-project/jackin"},
            "head_repository": {"id": 9, "full_name": "jackin-project/jackin"}
        }
    })
}

fn selector_event() -> Value {
    let mut event = event();
    event["repository"]["full_name"] = json!("example/app");
    event["workflow_run"]["repository"]["full_name"] = json!("example/app");
    event["workflow_run"]["head_repository"]["full_name"] = json!("example/app");
    event
}

fn policy() -> PackageReleasePolicy {
    serde_json::from_value(json!({
        "schema": "velnor.package-release-policy.v1",
        "channel": "preview",
        "repository": "example/app",
        "default_branch": "main",
        "version": {
            "version_package": "app",
            "bindings": [{
                "package": "app",
                "manifest_path": "Cargo.toml",
                "workspace_manifest_path": "Cargo.toml"
            }]
        },
        "products": [],
        "companions": []
    }))
    .expect("minimal typed preview policy")
}

#[test]
fn completed_main_push_event_binds_source_to_admitted_ci_workflow() {
    assert_eq!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &workflow())
            .expect("authenticated event shape"),
        ("a".repeat(40), 101, 2)
    );
}

#[test]
fn source_event_rejects_each_independent_workflow_identity_mutation() {
    for (pointer, replacement) in [
        ("/action", json!("requested")),
        ("/repository/id", json!(10)),
        ("/repository/full_name", json!("attacker/fork")),
        ("/workflow_run/repository/id", json!(10)),
        ("/workflow_run/repository/full_name", json!("attacker/fork")),
        ("/workflow/id", json!(8)),
        ("/workflow/name", json!("Untrusted CI")),
        ("/workflow/path", json!(".github/workflows/other.yml")),
        ("/workflow_run/workflow_id", json!(8)),
        ("/workflow_run/head_repository/id", json!(10)),
        (
            "/workflow_run/head_repository/full_name",
            json!("attacker/fork"),
        ),
        ("/workflow_run/head_branch", json!("feature")),
        ("/workflow_run/event", json!("pull_request")),
        ("/workflow_run/status", json!("in_progress")),
        ("/workflow_run/conclusion", json!("failure")),
        ("/workflow_run/path", json!(".github/workflows/other.yml")),
        ("/workflow_run/name", json!("Untrusted CI")),
        ("/workflow_run/head_sha", json!("A".repeat(40))),
    ] {
        let mut candidate = event();
        *candidate.pointer_mut(pointer).expect("mutation field") = replacement;
        assert!(
            source_from_workflow_run_event(&candidate, 9, "jackin-project/jackin", &workflow())
                .is_err(),
            "accepted modified field {pointer}"
        );
    }
}

#[test]
fn source_event_rejects_malformed_envelopes_and_unadmitted_workflow_metadata() {
    for candidate in [
        Value::Null,
        json!({}),
        json!({"action":"completed","workflow_run":null}),
        {
            let mut candidate = event();
            candidate["workflow"] = Value::Null;
            candidate
        },
        {
            let mut candidate = event();
            candidate["workflow_run"] = json!({});
            candidate
        },
    ] {
        assert!(
            source_from_workflow_run_event(&candidate, 9, "jackin-project/jackin", &workflow())
                .is_err(),
            "accepted malformed event envelope"
        );
    }

    let mut wrong_workflow = workflow();
    wrong_workflow.id = 0;
    assert!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &wrong_workflow)
            .is_err()
    );
    let mut wrong_workflow = workflow();
    wrong_workflow.path = ".github/workflows/other.yml".to_owned();
    assert!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &wrong_workflow)
            .is_err()
    );
    let mut wrong_workflow = workflow();
    wrong_workflow.name = "Untrusted CI".to_owned();
    assert!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &wrong_workflow)
            .is_err()
    );
}

#[test]
fn selector_context_accepts_only_the_preview_authority_and_supported_triggers() {
    let seed = policy();
    validate_preview_source_context(
        &seed,
        "example/app",
        &"a".repeat(40),
        &"a".repeat(40),
        "refs/heads/main",
        "example/app/.github/workflows/binary-preview.yml@refs/heads/main",
        "workflow_run",
    )
    .expect("valid selector context");
}

#[test]
fn selector_context_rejects_forbidden_triggers() {
    let seed = policy();
    for event_name in [
        "pull_request",
        "pull_request_target",
        "push",
        "repository_dispatch",
    ] {
        let error = validate_preview_source_context(
            &seed,
            "example/app",
            &"a".repeat(40),
            &"a".repeat(40),
            "refs/heads/main",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main",
            event_name,
        )
        .expect_err("forbidden trigger");
        assert!(format!("{error:?}").contains("preview_source_event_name"));
    }
}

#[test]
fn selector_context_rejects_untrusted_authority_mutations() {
    let seed = policy();
    for (repository, authority, workflow_sha, ref_name, workflow_ref, candidate_seed) in [
        (
            "attacker/fork",
            "a".repeat(40),
            "a".repeat(40),
            "refs/heads/main",
            "attacker/fork/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            seed.clone(),
        ),
        (
            "example/app",
            "a".repeat(40),
            "b".repeat(40),
            "refs/heads/main",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            seed.clone(),
        ),
        (
            "example/app",
            "a".repeat(40),
            "a".repeat(40),
            "refs/heads/feature",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            seed.clone(),
        ),
        (
            "example/app",
            "a".repeat(40),
            "a".repeat(40),
            "refs/heads/main",
            "example/app/.github/workflows/other.yml@refs/heads/main".to_owned(),
            seed.clone(),
        ),
        (
            "example/app",
            "a".repeat(40),
            "a".repeat(40),
            "refs/heads/main",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            {
                let mut candidate = seed.clone();
                candidate.default_branch = "release".to_owned();
                candidate
            },
        ),
        (
            "example/app",
            "a".repeat(40),
            "a".repeat(40),
            "refs/heads/main",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            {
                let mut candidate = seed.clone();
                candidate.channel = PackageReleaseChannel::Stable;
                candidate
            },
        ),
    ] {
        let error = validate_preview_source_context(
            &candidate_seed,
            repository,
            &authority,
            &workflow_sha,
            ref_name,
            &workflow_ref,
            "workflow_dispatch",
        )
        .expect_err("mismatched authority context");
        assert!(format!("{error:?}").contains("preview_source_authority_context"));
    }
}

fn manual_run() -> Value {
    json!({
        "id":101,
        "run_attempt":2,
        "head_sha":"a".repeat(40),
        "head_branch":"main",
        "event":"push",
        "status":"completed",
        "conclusion":"success",
        "repository":{"id":9},
        "head_repository":{"id":9}
    })
}

#[test]
fn workflow_dispatch_requires_canonical_run_attempt_and_exact_successful_main_run() {
    for (id, attempt) in [
        ("", "2"),
        ("0", "2"),
        ("01", "2"),
        ("101", ""),
        ("101", "0"),
        ("101", "02"),
        ("101", "4294967296"),
    ] {
        assert!(parse_manual_run_identity(id, attempt).is_err());
    }
    assert_eq!(
        parse_manual_run_identity("101", "2").expect("canonical identity"),
        (101, 2)
    );

    let route = "repos/example/app/actions/runs/101/attempts/2";
    let (api, fixture) =
        GitHubApi::with_fixture("example/app", vec![(route.to_owned(), manual_run())]);
    assert_eq!(
        source_from_workflow_dispatch(&api, 9, 101, 2).expect("admitted manual source"),
        ("a".repeat(40), 101, 2)
    );
    fixture.assert_drained();

    for (pointer, replacement) in [
        ("/id", json!(102)),
        ("/run_attempt", json!(3)),
        ("/head_branch", json!("feature")),
        ("/event", json!("pull_request")),
        ("/status", json!("in_progress")),
        ("/conclusion", json!("failure")),
        ("/repository/id", json!(10)),
        ("/head_repository/id", json!(10)),
        ("/head_sha", json!("A".repeat(40))),
    ] {
        let mut candidate = manual_run();
        *candidate.pointer_mut(pointer).expect("identity field") = replacement;
        let (api, fixture) =
            GitHubApi::with_fixture("example/app", vec![(route.to_owned(), candidate)]);
        assert!(
            source_from_workflow_dispatch(&api, 9, 101, 2).is_err(),
            "accepted manual run mutation {pointer}"
        );
        fixture.assert_drained();
    }
}

#[test]
fn selector_authentication_failure_never_emits_the_checkout_gate_output() {
    assert_eq!(
        source_from_workflow_run_event(&selector_event(), 9, "example/app", &workflow())
            .expect("valid selector event baseline"),
        ("a".repeat(40), 101, 2)
    );
    for pointer in [
        "/workflow/id",
        "/workflow/name",
        "/workflow/path",
        "/workflow_run/workflow_id",
    ] {
        let mut candidate = selector_event();
        *candidate.pointer_mut(pointer).expect("workflow field") = match pointer {
            "/workflow/id" | "/workflow_run/workflow_id" => json!(8),
            "/workflow/name" => json!("Untrusted CI"),
            _ => json!(".github/workflows/other.yml"),
        };
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
            ],
        );
        let mut output = String::from("preserve-existing-output\n");
        let mut checkout_gate_reached = false;
        let deadline = CheckDeadline::after(Duration::from_secs(30)).expect("deadline");
        let result = select_and_emit_source(
            &api,
            SelectionRequest {
                event: &candidate,
                seed: &policy(),
                repository_id: 9,
                authority: &"b".repeat(40),
                current_run: 202,
                current_attempt: 1,
                event_name: "workflow_run",
                job_name: "preview-eligibility",
                manual_run_identity: None,
                deadline,
            },
            |value| {
                output.push_str(value);
                checkout_gate_reached = true;
                Ok(())
            },
        );
        assert!(
            format!("{result:?}").contains("preview_source_event_identity"),
            "mutation {pointer} reached the wrong rejection branch: {result:?}"
        );
        assert_eq!(output, "preserve-existing-output\n");
        assert!(!checkout_gate_reached);
        fixture.assert_drained();
        assert_eq!(
            fixture.observed(),
            [
                "repos/example/app",
                "repos/example/app/actions/workflows/ci.yml"
            ]
        );
    }
}
