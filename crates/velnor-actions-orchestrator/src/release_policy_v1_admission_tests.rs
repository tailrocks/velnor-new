use super::*;
use serde_json::json;

fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: velnor_actions_contract::PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "1.2.3".to_owned(),
        products: Vec::new(),
        companions: Vec::new(),
    }
}

fn context() -> ReleaseProductionContext {
    ReleaseProductionContext {
        repository_id: 9,
        head_repository_id: 9,
        source_sha: "a".repeat(40),
        source_ref: "refs/heads/main".to_owned(),
        workflow_authority_sha: "a".repeat(40),
        run_id: 202,
        run_attempt: 1,
        ci_run_id: 101,
        ci_run_attempt: 2,
        commit_count: 8,
        version: "1.2.3-preview.8+aaaaaaaa".to_owned(),
    }
}

fn workflow_ref(plan: &PackageReleasePlan) -> String {
    format!(
        "{}/{}@refs/heads/main",
        plan.repository,
        plan.channel.workflow_path()
    )
}

#[test]
fn v1_operation_keeps_workflow_run_and_same_sha_authority_contract() {
    let plan = plan();
    let sha = "a".repeat(40);
    let workflow = workflow_ref(&plan);

    assert!(legacy_workflow_identity_matches(
        &plan,
        "workflow_run",
        "example/app",
        "refs/heads/main",
        &sha,
        &sha,
        &workflow,
    ));
    assert!(!legacy_workflow_identity_matches(
        &plan,
        "workflow_dispatch",
        "example/app",
        "refs/heads/main",
        &sha,
        &sha,
        &workflow,
    ));
    assert!(!legacy_workflow_identity_matches(
        &plan,
        "workflow_run",
        "example/app",
        "refs/heads/main",
        &sha,
        &"b".repeat(40),
        &workflow,
    ));
}

#[test]
fn v1_event_identity_requires_the_original_completed_ci_run() {
    let plan = plan();
    let context = context();
    let event = json!({
        "action": "completed",
        "repository": {"id": 9, "full_name": "example/app"},
        "workflow_run": {
            "id": 101,
            "run_attempt": 2,
            "head_repository": {"id": 9},
            "head_sha": "a".repeat(40),
            "head_branch": "main",
            "event": "push",
            "status": "completed",
            "conclusion": "success",
            "path": velnor_actions_contract::CI_WORKFLOW_PATH,
            "name": velnor_actions_contract::WORKFLOW_DISPLAY_NAME
        }
    });

    assert!(legacy_event_matches(&plan, &context, &event));

    let mut changed = event.clone();
    changed["workflow_run"]["head_sha"] = json!("b".repeat(40));
    assert!(!legacy_event_matches(&plan, &context, &changed));

    let mut changed = event;
    changed["workflow_run"]["event"] = json!("workflow_dispatch");
    assert!(!legacy_event_matches(&plan, &context, &changed));
}
