use super::super::super::github::WorkflowMetadata;
use super::selection::{
    SelectionRequest, select_and_emit_source, source_from_workflow_run_event,
    validate_preview_source_context,
};
use super::*;
use serde_json::json;

fn workflow() -> WorkflowMetadata {
    WorkflowMetadata {
        id: 7,
        name: velnor_actions_contract::WORKFLOW_DISPLAY_NAME.to_owned(),
        path: velnor_actions_contract::CI_WORKFLOW_PATH.to_owned(),
        state: "active".to_owned(),
    }
}

fn event() -> Value {
    json!({
        "action": "completed",
        "repository": {"id": 9, "full_name": "jackin-project/jackin"},
        "workflow": {
            "id": 7,
            "name": velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
            "path": velnor_actions_contract::CI_WORKFLOW_PATH
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
            "path": velnor_actions_contract::CI_WORKFLOW_PATH,
            "name": velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
            "repository": {"id": 9, "full_name": "jackin-project/jackin"},
            "head_repository": {"id": 9, "full_name": "jackin-project/jackin"}
        }
    })
}

fn selector_event() -> Value {
    let mut candidate = event();
    candidate["repository"]["full_name"] = json!("example/app");
    candidate["workflow_run"]["repository"]["full_name"] = json!("example/app");
    candidate["workflow_run"]["head_repository"]["full_name"] = json!("example/app");
    candidate
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
fn completed_main_push_ci_event_admits_source_identity_a() {
    assert_eq!(
        source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &workflow())
            .expect("authenticated event shape"),
        ("a".repeat(40), 101, 2)
    );
}

#[test]
fn source_event_rejects_foreign_repositories_wrong_workflow_and_non_success() {
    for (pointer, replacement) in [
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
            "accepted modified source event"
        );
    }
}

#[test]
fn source_event_rejects_malformed_envelopes_and_unadmitted_workflow_metadata() {
    let malformed = [
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
    ];
    for candidate in malformed {
        assert!(
            source_from_workflow_run_event(&candidate, 9, "jackin-project/jackin", &workflow())
                .is_err(),
            "accepted malformed event envelope"
        );
    }

    for candidate in [
        WorkflowMetadata {
            id: 0,
            ..workflow()
        },
        WorkflowMetadata {
            path: ".github/workflows/other.yml".to_owned(),
            ..workflow()
        },
        WorkflowMetadata {
            name: "Untrusted CI".to_owned(),
            ..workflow()
        },
    ] {
        assert!(
            source_from_workflow_run_event(&event(), 9, "jackin-project/jackin", &candidate)
                .is_err(),
            "accepted unadmitted CI workflow metadata"
        );
    }
}

#[test]
fn selector_event_identity_failures_do_not_emit_checkout_gate_output() {
    assert_eq!(
        source_from_workflow_run_event(&selector_event(), 9, "example/app", &workflow())
            .expect("valid selector event baseline"),
        ("a".repeat(40), 101, 2)
    );

    for (pointer, replacement) in [
        ("/workflow/id", json!(8)),
        ("/workflow/name", json!("Untrusted CI")),
        ("/workflow/path", json!(".github/workflows/other.yml")),
        ("/workflow_run/workflow_id", json!(8)),
    ] {
        let mut candidate = selector_event();
        *candidate
            .pointer_mut(pointer)
            .expect("workflow identity field") = replacement;
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
                        "name":velnor_actions_contract::WORKFLOW_DISPLAY_NAME,
                        "path":velnor_actions_contract::CI_WORKFLOW_PATH,
                        "state":"active"
                    }),
                ),
            ],
        );
        let seed = policy();
        let deadline = CheckDeadline::after(Duration::from_secs(30)).expect("deadline");
        let mut output = String::from("preserve-existing-output\n");
        let mut checkout_gate_reached = false;
        let result = select_and_emit_source(
            &api,
            &SelectionRequest {
                event: &candidate,
                seed: &seed,
                repository_id: 9,
                authority: &"b".repeat(40),
                current_run: 202,
                current_attempt: 1,
                event_name: "workflow_run",
                job_name: "preview-eligibility",
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

#[test]
fn selector_context_accepts_only_preview_authority_and_supported_triggers() {
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
    let mutations = [
        (
            "attacker/fork",
            "a",
            "a",
            "refs/heads/main",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            seed.clone(),
        ),
        (
            "example/app",
            "a",
            "b",
            "refs/heads/main",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            seed.clone(),
        ),
        (
            "example/app",
            "a",
            "a",
            "refs/heads/feature",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            seed.clone(),
        ),
        (
            "example/app",
            "a",
            "a",
            "refs/heads/main",
            "example/app/.github/workflows/other.yml@refs/heads/main".to_owned(),
            seed.clone(),
        ),
        (
            "example/app",
            "a",
            "a",
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
            "a",
            "a",
            "refs/heads/main",
            "example/app/.github/workflows/binary-preview.yml@refs/heads/main".to_owned(),
            {
                let mut candidate = seed.clone();
                candidate.channel = PackageReleaseChannel::Stable;
                candidate
            },
        ),
    ];
    for (repository, authority, workflow_sha, ref_name, workflow_ref, candidate_seed) in mutations {
        let error = validate_preview_source_context(
            &candidate_seed,
            repository,
            &authority.repeat(40),
            &workflow_sha.repeat(40),
            ref_name,
            &workflow_ref,
            "workflow_dispatch",
        )
        .expect_err("mismatched authority context");
        assert!(format!("{error:?}").contains("preview_source_authority_context"));
    }
}

#[test]
fn source_checkout_lfs_uses_the_authenticated_config_and_defaults_off() {
    assert!(!config_checkout_lfs(None).expect("absent source config default"));
    assert!(
        config_checkout_lfs(Some(
            "schema = 1\n[workflow]\nname = \"CI\"\ncheckout_lfs = true\n"
        ))
        .expect("source config value")
    );
    assert!(
        !config_checkout_lfs(Some(
            "schema = 1\n[workflow]\nname = \"CI\"\ncheckout_lfs = false\n"
        ))
        .expect("explicit source config false")
    );
    assert!(
        config_checkout_lfs(Some(
            "schema = 1\n[workflow]\nname = \"CI\"\ncheckout_lfs = \"true\"\n"
        ))
        .is_err()
    );
}
