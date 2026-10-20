//! Actual process-environment admission using canonical receipt ref semantics.
use super::*;
use std::process::Command;

#[test]
fn terminal_publisher_and_direct_tap_admit_real_workflow_refs_and_reject_substitutions() {
    if let Ok(case) = std::env::var("VELNOR_DIRECT_CALLER_FIXTURE") {
        child(&case);
        return;
    }
    let name = "package_release_publish::run_identity::transport_tests::direct_caller::terminal_publisher_and_direct_tap_admit_real_workflow_refs_and_reject_substitutions";
    for case in [
        "publisher",
        "prepare",
        "writer",
        "merge",
        "short_ref",
        "other_workflow",
        "wrong_source",
        "unknown_job",
        "cancelled_job",
        "new_attempt",
        "other_run",
    ] {
        let output = Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", name, "--nocapture"])
            .env("VELNOR_DIRECT_CALLER_FIXTURE", case)
            .env("GITHUB_REPOSITORY", "example/app")
            .env("GITHUB_REPOSITORY_ID", "9")
            .env(
                "GITHUB_RUN_ID",
                if case == "other_run" { "23" } else { "22" },
            )
            .env(
                "GITHUB_RUN_ATTEMPT",
                if case == "new_attempt" { "4" } else { "3" },
            )
            .env("GITHUB_SHA", source().sha)
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_WORKFLOW_SHA", source().sha)
            .env(
                "GITHUB_WORKFLOW_REF",
                match case {
                    "short_ref" => "refs/heads/main",
                    "other_workflow" => "example/app/.github/workflows/other.yml@refs/heads/main",
                    _ => "example/app/.github/workflows/binary-preview.yml@refs/heads/main",
                },
            )
            .env("GITHUB_EVENT_NAME", "workflow_run")
            .env(
                "GITHUB_JOB",
                match case {
                    "prepare" => "preview-tap-prepare",
                    "writer" => "preview-tap-writer",
                    "merge" => "preview-tap-merge",
                    "unknown_job" => "preview-other",
                    _ => "preview-publisher",
                },
            )
            .output()
            .expect("isolated real environment admission");
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn child(case: &str) {
    let mut receipt = release_receipt();
    if case == "wrong_source" {
        receipt.source.commit_sha = "b".repeat(40);
    }
    let plan = crate::package_release_publish::preview_artifacts::fixture::request().plan;
    let admitted = matches!(case, "publisher" | "prepare" | "writer" | "merge");
    let reaches_api = admitted || case == "cancelled_job";
    let role = match case {
        "prepare" => "Prepare a literal-only tap update without write credentials",
        "writer" => "Independently validate and create a signed-off tap PR",
        "merge" => "Merge only the validated and fully reviewed PR head",
        _ => "Publish authenticated channel release",
    };
    let responses = if reaches_api {
        vec![
            (
                route("actions/workflows/binary-preview.yml"),
                producing_metadata(),
            ),
            (route("actions/runs/22/attempts/3"), producing_run()),
            (route("actions/runs/22"), producing_run()),
            (
                route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
                json!({"total_count":1,"jobs":[{"id":81,"run_id":22,"head_sha":source().sha,"name":role,"status":if case=="cancelled_job" {"completed"} else {"in_progress"},"conclusion":null,"started_at":"2026-10-20T20:54:22Z"}]}),
            ),
        ]
    } else {
        vec![]
    };
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    let result = api.verify_direct_release_caller(&plan, &receipt);
    if admitted {
        result.expect("canonical direct caller is admitted");
    } else {
        result.expect_err("substituted caller is refused");
    }
    fixture.assert_drained();
}
