//! Generated continuation identity is admitted by the real runtime API boundary.
use super::*;
use serde_json::{Value, json};

const CHILD: &str = "VELNOR_TAP_IDENTITY_CHILD";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ROLES: [&str; 4] = ["selection", "prepare", "writer", "merge"];

fn route(path: &str) -> String {
    format!("repos/example/app/{path}")
}

fn generated_identity(
    channel: PackageReleaseChannel,
) -> (String, String, BTreeMap<String, String>) {
    let spec = fixture(channel);
    let workflow = render_release_tap_reconcile(&spec).expect("real continuation render");
    let name = workflow
        .bytes
        .lines()
        .find_map(|line| line.strip_prefix("name: "))
        .expect("workflow display name")
        .trim_matches(['\"', '\''])
        .to_owned();
    (workflow.path, name, job_names(&workflow.bytes))
}

#[test]
fn rendered_producer_metadata_is_admitted_by_the_runtime_identity_api() {
    for channel in [
        PackageReleaseChannel::Preview,
        PackageReleaseChannel::Stable,
    ] {
        let spec = fixture(channel);
        let rendered = match channel {
            PackageReleaseChannel::Preview => render_consumer_preview(&spec),
            PackageReleaseChannel::Stable => render_consumer_binary_release(&spec),
        }
        .expect("real release workflow render");
        let name = rendered
            .bytes
            .lines()
            .find_map(|line| line.strip_prefix("name: "))
            .expect("workflow display name")
            .trim_matches(['\"', '\'']);
        let filename = rendered.path.rsplit('/').next().expect("workflow filename");
        let metadata = json!({
            "id": 7,
            "name": name,
            "path": rendered.path,
            "state": "active"
        });
        let (api, responses) = crate::package_release_publish::github::GitHubApi::with_fixture(
            "example/app",
            vec![(route(&format!("actions/workflows/{filename}")), metadata)],
        );
        api.admitted_workflow(channel.workflow_path(), channel.workflow_name())
            .expect("renderer output matches exact runtime workflow identity");
        responses.assert_drained();
    }
}

#[derive(Clone, Copy)]
struct ChildRun<'a> {
    test_name: &'a str,
    channel: &'a str,
    event: &'a str,
    job: &'a str,
    mutation: &'a str,
    path: &'a str,
    name: &'a str,
}

fn channel(value: &str) -> PackageReleaseChannel {
    match value {
        "preview" => PackageReleaseChannel::Preview,
        "stable" => PackageReleaseChannel::Stable,
        _ => panic!("test channel is closed"),
    }
}

fn run(path: &str, name: &str, event: &str, mutation: &str) -> Value {
    let mut run = json!({
        "id": 22,
        "run_attempt": 3,
        "workflow_id": 7,
        "name": name,
        "path": path,
        "head_sha": SHA,
        "head_branch": "main",
        "event": event,
        "status": "in_progress",
        "conclusion": null,
        "repository": {"id": 9, "full_name": "example/app"},
        "head_repository": {"id": 9, "full_name": "example/app"}
    });
    match mutation {
        "wrong_run_name" => run["name"] = json!("Foreign workflow"),
        "wrong_run_attempt" => run["run_attempt"] = json!(4),
        "wrong_run_sha" => run["head_sha"] = json!("b".repeat(40)),
        "wrong_run_repository" => {
            run["repository"]["id"] = json!(10);
            run["head_repository"]["id"] = json!(10);
        }
        "wrong_run_event" => run["event"] = json!("push"),
        "terminal_run" => {
            run["status"] = json!("completed");
            run["conclusion"] = json!("success");
        }
        _ => {}
    }
    run
}

#[test]
fn rendered_continuation_identity_authenticates_and_rejects_substitutions() {
    if std::env::var_os(CHILD).is_some() {
        child();
        return;
    }

    let test_name = concat!(
        "package_release_publish::run_identity::active_role::rendered_tests::tap_identity_tests::",
        "rendered_continuation_identity_authenticates_and_rejects_substitutions"
    );
    for channel_name in ["preview", "stable"] {
        let channel = channel(channel_name);
        let (path, name, jobs) = generated_identity(channel);
        let prefix = channel_name;
        let role_ids = ROLES.map(|role| format!("{prefix}-tap-{role}"));
        for event in ["schedule", "workflow_dispatch"] {
            for role in &role_ids {
                assert!(jobs.contains_key(role), "generated role missing: {role}");
                run_child(ChildRun {
                    test_name,
                    channel: channel_name,
                    event,
                    job: role,
                    mutation: "",
                    path: &path,
                    name: &name,
                });
            }
        }
        for mutation in [
            "wrong_workflow_ref",
            "lowercase_metadata_name",
            "wrong_metadata_path",
            "disabled_workflow",
            "zero_workflow_id",
            "wrong_run_name",
            "wrong_run_attempt",
            "wrong_run_sha",
            "wrong_run_repository",
            "wrong_run_event",
            "unknown_job",
            "terminal_run",
        ] {
            run_child(ChildRun {
                test_name,
                channel: channel_name,
                event: "schedule",
                job: &role_ids[0],
                mutation,
                path: &path,
                name: &name,
            });
        }
    }
}

fn run_child(case: ChildRun<'_>) {
    let workflow_ref = if case.mutation == "wrong_workflow_ref" {
        "example/app/.github/workflows/foreign.yml@refs/heads/main".to_owned()
    } else {
        format!("example/app/{}@refs/heads/main", case.path)
    };
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", case.test_name, "--nocapture"])
        .env(CHILD, "1")
        .env("VELNOR_TAP_CHANNEL", case.channel)
        .env("VELNOR_TAP_MUTATION", case.mutation)
        .env("GITHUB_REPOSITORY", "example/app")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_SHA", SHA)
        .env("GITHUB_WORKFLOW_SHA", SHA)
        .env("GITHUB_WORKFLOW_REF", workflow_ref)
        .env("GITHUB_EVENT_NAME", case.event)
        .env("GITHUB_RUN_ID", "22")
        .env("GITHUB_RUN_ATTEMPT", "3")
        .env(
            "GITHUB_JOB",
            if case.mutation == "unknown_job" {
                if case.channel == "preview" {
                    "preview-tap-unknown"
                } else {
                    "stable-tap-unknown"
                }
            } else {
                case.job
            },
        )
        .env("VELNOR_TAP_RENDERED_PATH", case.path)
        .env("VELNOR_TAP_RENDERED_NAME", case.name)
        .output()
        .expect("isolated authenticated continuation child");
    assert!(
        output.status.success(),
        "{} {} {} {}: {}",
        case.channel,
        case.event,
        case.job,
        case.mutation,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn child() {
    let channel_name = std::env::var("VELNOR_TAP_CHANNEL").expect("channel");
    let channel = channel(&channel_name);
    let mutation = std::env::var("VELNOR_TAP_MUTATION").expect("mutation");
    let event = std::env::var("GITHUB_EVENT_NAME").expect("event");
    let job_id = std::env::var("GITHUB_JOB").expect("job");
    let (path, name, jobs) = generated_identity(channel);
    assert_eq!(
        path,
        std::env::var("VELNOR_TAP_RENDERED_PATH").expect("path")
    );
    assert_eq!(
        name,
        std::env::var("VELNOR_TAP_RENDERED_NAME").expect("name")
    );
    let expected_job = continuation_job_name(channel, &job_id).ok();
    if mutation == "unknown_job" {
        assert!(expected_job.is_none());
    } else {
        let expected_job = expected_job.expect("generated continuation role");
        assert_eq!(jobs.get(&job_id).map(String::as_str), Some(expected_job));
    }

    let filename = path.rsplit('/').next().expect("workflow filename");
    let mut metadata = json!({"id":7,"name":name,"path":path,"state":"active"});
    match mutation.as_str() {
        "lowercase_metadata_name" => metadata["name"] = json!(name.to_ascii_lowercase()),
        "wrong_metadata_path" => metadata["path"] = json!(".github/workflows/foreign.yml"),
        "disabled_workflow" => metadata["state"] = json!("disabled_manually"),
        "zero_workflow_id" => metadata["id"] = json!(0),
        _ => {}
    }
    let mut responses = if mutation == "wrong_workflow_ref" {
        Vec::new()
    } else {
        vec![(route(&format!("actions/workflows/{filename}")), metadata)]
    };
    if !matches!(
        mutation.as_str(),
        "wrong_workflow_ref"
            | "lowercase_metadata_name"
            | "wrong_metadata_path"
            | "disabled_workflow"
            | "zero_workflow_id"
            | "unknown_job"
    ) {
        let attempt = run(&path, &name, &event, &mutation);
        responses.push((route("actions/runs/22/attempts/3"), attempt.clone()));
        responses.push((route("actions/runs/22"), attempt));
        if mutation.is_empty() {
            responses.push((
                route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
                json!({"total_count":1,"jobs":[{"id":81,"run_id":22,"name":expected_job.expect("valid generated role"),"head_sha":SHA,"status":"in_progress","conclusion":null,"started_at":"2026-10-24T00:00:00Z"}]}),
            ));
        }
    }
    let (api, api_fixture) =
        crate::package_release_publish::github::GitHubApi::with_fixture("example/app", responses);
    let invocation =
        crate::package_release_publish::recovery::promotion_context::Invocation::from_env()
            .expect("canonical process context");
    let result = invocation.authenticate_continuation(
        &fixture(channel)
            .policy
            .resolve("0.6.4".to_owned())
            .expect("plan"),
        &api,
        9,
    );
    if mutation.is_empty() {
        result.expect("generated workflow identity and current role are admitted");
    } else {
        result.expect_err("identity substitution or invalid role is rejected");
    }
    api_fixture.assert_drained();
}
