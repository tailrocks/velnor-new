use super::*;

fn jobs_page(jobs: &serde_json::Value, total_count: usize) -> serde_json::Value {
    json!({"total_count":total_count,"jobs":jobs})
}

fn list_route_page(page: usize) -> String {
    route(&format!(
        "actions/workflows/7/runs?head_sha={}&per_page=100&page={page}",
        source().sha
    ))
}

fn inventory_run(id: u64, run_number: u64) -> serde_json::Value {
    let mut run = run_json();
    run["id"] = json!(id);
    run["run_number"] = json!(run_number);
    run
}

#[test]
fn ci_inventory_page_caps_fail_closed() {
    let jobs = (0..100)
        .map(|n| {
            json!({"id":100+n,"run_id":22,"name":format!("Other {n}"),"head_sha":source().sha,"status":"completed","conclusion":"success"})
        })
        .collect::<Vec<_>>();
    let responses = first_three(jobs_page(&json!(jobs), 2_001));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(api.verify_ci_identity(22, 3, source(), false).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 3);

    let first_page = (0..100)
        .map(|n| {
            json!({"id":100+n,"run_id":22,"name":format!("Other {n}"),"head_sha":source().sha,"status":"completed","conclusion":"success"})
        })
        .collect::<Vec<_>>();
    let mut responses = first_three(jobs_page(&json!(first_page), 2_000));
    for page in 2..=20 {
        let jobs = (0..100)
            .map(|n| {
                let id = page * 100 + n;
                json!({"id":id,"run_id":22,"name":format!("Other {id}"),"head_sha":source().sha,"status":"completed","conclusion":"success"})
            })
            .collect::<Vec<_>>();
        responses.push((
            route(&format!(
                "actions/runs/22/attempts/3/jobs?per_page=100&page={page}"
            )),
            jobs_page(&json!(jobs), 2_000),
        ));
    }
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(api.verify_ci_identity(22, 3, source(), false).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 22);

    let mut responses = first_three(json!({"total_count":1,"jobs":[required()]}));
    for page in 1..=10 {
        responses.push((
            list_route_page(page),
            json!({
                "workflow_runs": (0..100)
                    .map(|index| {
                        let run_number = ((page - 1) * 100 + index + 1) as u64;
                        inventory_run(1_000 + run_number, run_number)
                    })
                    .collect::<Vec<_>>()
            }),
        ));
    }
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 13);
}
