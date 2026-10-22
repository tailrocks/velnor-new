use super::*;

#[test]
fn empty_listing_then_success_returns_the_run_after_one_backoff() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (list_route(), json!({"workflow_runs": []})),
            (list_route(), json!({"workflow_runs": []})),
            (list_route(), json!({"workflow_runs": [run_value(22, 12)]})),
        ],
    );
    let mut waits = Vec::new();

    let latest = api
        .latest_source_ci_with_sleep(&workflow(), source(), |duration| {
            waits.push(duration);
            Ok(())
        })
        .expect("a run that appears after empty listings is admitted");

    assert_eq!(latest.id, Some(22));
    fixture.assert_drained();
    assert_eq!(waits, [3, 6].map(Duration::from_secs));
}

#[test]
fn listing_api_error_is_terminal_without_retry() {
    let mut reads = 0;
    let mut waits = Vec::new();
    let result = retry_empty_workflow_run_inventory(
        |_| {
            reads += 1;
            Err(internal("github_api_request"))
        },
        |duration| {
            waits.push(duration);
            Ok(())
        },
        check,
    );
    assert!(result.is_err());
    assert_eq!(reads, 1);
    assert_eq!(waits, Vec::<Duration>::new());

    // Through the real adapter: no queued response is an API failure.
    let (api, fixture) = GitHubApi::with_fixture("example/app", Vec::new());
    let waits = Cell::new(0);
    let Err(error) = api.latest_source_ci_with_sleep(&workflow(), source(), |_| {
        waits.set(waits.get() + 1);
        Ok(())
    }) else {
        panic!("an API failure must not be retried into success");
    };
    assert!(error.to_string().contains("unexpected_fixture_api_call"));
    assert_eq!(fixture.observed(), [list_route()]);
    assert_eq!(waits.get(), 0);
}

#[test]
fn empty_latest_attempt_recheck_retries_with_the_injected_sleeper() {
    let required = json!({"total_count":1,"jobs":[{"id":81,"run_id":22,"name":"Required",
        "head_sha":SOURCE_SHA,"status":"completed","conclusion":"success"}]});
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                route("actions/workflows/ci.yml"),
                json!({"id":7,"name":"CI","path":".github/workflows/ci.yml","state":"active"}),
            ),
            (route("actions/runs/22/attempts/1"), run_value(22, 12)),
            (
                route("actions/runs/22/attempts/1/jobs?per_page=100&page=1"),
                required,
            ),
            (list_route(), json!({"workflow_runs": []})),
            (list_route(), json!({"workflow_runs": [run_value(22, 12)]})),
            (route("actions/runs/22"), run_value(22, 12)),
        ],
    );
    let mut waits = Vec::new();

    api.verify_ci_identity_with_sleep(22, 1, source(), true, |duration| {
        waits.push(duration);
        Ok(())
    })
    .expect("recheck admitted after one empty listing");

    fixture.assert_drained();
    assert_eq!(waits, [Duration::from_secs(3)]);
}
