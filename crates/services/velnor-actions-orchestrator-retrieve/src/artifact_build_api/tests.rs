use super::*;

#[test]
fn api_page_parser_rejects_duplicate_keys_and_keeps_only_declared_fields() {
    let valid = r#"{"total_count":1,"jobs":[{"id":7,"run_id":8,"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","status":"completed","conclusion":"success","name":"Build artifact github_hosted / bundle","runner_id":null,"runner_name":null,"runner_group_id":null,"runner_group_name":null,"labels":["ubuntu-26.04"],"steps":[{"name":"large omitted field"}]}]}"#;
    let page: JobsPage = parse_page(valid).expect("minimal filtered job page");
    assert_eq!(page.total_count, 1);
    assert_eq!(page.jobs[0].id, 7);
    assert_eq!(page.jobs[0].labels, ["ubuntu-26.04"]);
    assert!(parse_page::<JobsPage>(r#"{"total_count":0,"total_count":1,"jobs":[]}"#).is_err());
}

#[test]
fn api_arguments_are_explicit_and_page_inventory_is_exact() {
    let args = api_args(
        "repos/o/r/actions/runs/7/attempts/2/jobs?per_page=100&page=3",
        "{total_count, jobs: [.jobs[] | {id, run_id, head_sha, status, conclusion, name, check_run_url, runner_id, runner_name, runner_group_id, runner_group_name, labels}]}",
    );
    let strings: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
    assert_eq!(strings[0], "api");
    assert_eq!(
        strings[1],
        "repos/o/r/actions/runs/7/attempts/2/jobs?per_page=100&page=3"
    );
    assert_eq!(strings[2], "--jq");
    assert!(strings[3].contains("runner_group_id"));
    assert!(strings[3].contains("check_run_url"));

    let mut requested = Vec::new();
    let values = collect_pages(
        |page| {
            requested.push(page);
            let start = (page - 1) * PER_PAGE + 1;
            let end = (start + PER_PAGE - 1).min(205);
            Ok((205, (start..=end).collect()))
        },
        |id: &usize| u64::try_from(*id).expect("bounded test ID"),
    )
    .expect("three complete pages");
    assert_eq!(requested, [1, 2, 3]);
    assert_eq!(values.len(), 205);
    assert_eq!(values[204], 205);
}

#[test]
fn api_page_inventory_rejects_truncation_and_duplicate_ids() {
    let truncated = collect_pages(|_| Ok((101, vec![1_u64])), |id: &u64| *id);
    assert_eq!(
        truncated.expect_err("truncated page"),
        "actions_api_page_count_mismatch"
    );

    let duplicate = collect_pages(
        |page| {
            if page == 1 {
                Ok((2, vec![1_u64, 1_u64]))
            } else {
                Ok((2, Vec::new()))
            }
        },
        |id: &u64| *id,
    );
    assert_eq!(
        duplicate.expect_err("duplicate IDs"),
        "actions_api_duplicate_or_missing_id"
    );

    let excessive = collect_pages(|_| Ok((MAX_ITEMS + 1, Vec::<u64>::new())), |id: &u64| *id);
    assert_eq!(
        excessive.expect_err("oversized API inventory"),
        "actions_api_inventory_over_limit"
    );
}

fn api_job_with_check_run_url(check_run_url: Option<&str>) -> ApiJob {
    let mut job = serde_json::json!({
        "id": 77,
        "run_id": 8,
        "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "status": "completed",
        "conclusion": "success",
        "name": "Build artifact github_hosted / bundle",
        "runner_id": null,
        "runner_name": null,
        "runner_group_id": null,
        "runner_group_name": null,
        "labels": ["ubuntu-26.04"]
    });
    if let Some(url) = check_run_url {
        job["check_run_url"] = serde_json::json!(url);
    }
    let page = serde_json::json!({"total_count": 1, "jobs": [job]}).to_string();
    parse_page::<JobsPage>(&page)
        .expect("API job page")
        .jobs
        .into_iter()
        .next()
        .expect("one API job")
}

#[test]
fn check_run_urls_are_repository_scoped_positive_ids_and_absence_stays_distinct() {
    let repository = "ChainArgos/java-monorepo";
    let mut valid = api_job_with_check_run_url(Some(
        "https://api.github.com/repos/chainargos/JAVA-MONOREPO/check-runs/9021",
    ));
    valid
        .bind_check_run_id(repository)
        .expect("valid repository-scoped URL");
    assert_eq!(valid.check_run_id.map(NonZeroI64::get), Some(9021));

    let mut missing = api_job_with_check_run_url(None);
    missing
        .bind_check_run_id(repository)
        .expect("legacy missing URL");
    assert_eq!(missing.check_run_id, None);

    let invalid_urls = [
        "http://api.github.com/repos/ChainArgos/java-monorepo/check-runs/9",
        "https://api.github.com.evil/repos/ChainArgos/java-monorepo/check-runs/9",
        "https://api.github.com/repos/Other/java-monorepo/check-runs/9",
        "https://api.github.com/repos/ChainArgos/other-repo/check-runs/9",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/0",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/09",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/9?x=1",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/9/",
    ];
    for url in invalid_urls {
        let mut job = api_job_with_check_run_url(Some(url));
        assert!(
            job.bind_check_run_id(repository).is_err(),
            "invalid URL accepted: {url}"
        );
        assert_eq!(job.check_run_id, None);
    }
}
