use super::*;

fn job_with_check_run_url(id: i64, url: &str) -> String {
    job(id, RUN_ID, SHA).replace(
        "\"workflow_name\":",
        &format!("\"check_run_url\":\"{url}\",\"workflow_name\":"),
    )
}

#[test]
fn check_run_id_comes_from_scoped_url_and_missing_url_stays_absent() {
    let first = job_with_check_run_url(
        77,
        "https://api.github.com/repos/chainargos/JAVA-MONOREPO/check-runs/9021",
    );
    let page = jobs(2, &[first, job(78, RUN_ID, SHA)]);
    let run_body = run(2, SHA);
    let artifact_page = artifacts(0, &[]);
    let mut transport = scripted_transport(&[
        ("200", run_body.as_str()),
        ("200", page.as_str()),
        ("200", artifact_page.as_str()),
    ]);

    let result = block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
        &mut transport,
        OWNER,
        REPOSITORY,
        RUN_ID,
        2,
        SHA,
        TOKEN,
    ))
    .expect("provider inventory is readable");
    let Read::Complete(evidence) = result else {
        panic!("complete provider inventory expected");
    };
    assert_eq!(evidence.jobs[0].id, 77);
    assert_eq!(evidence.jobs[0].check_run_id, Some(9021));
    assert_ne!(Some(evidence.jobs[0].id), evidence.jobs[0].check_run_id);
    assert_eq!(evidence.jobs[1].check_run_id, None);
}

#[test]
fn foreign_or_malformed_check_run_urls_fail_without_partial_evidence() {
    let invalid_urls = [
        "http://api.github.com/repos/ChainArgos/java-monorepo/check-runs/9",
        "https://api.github.com.evil/repos/ChainArgos/java-monorepo/check-runs/9",
        "https://api.github.com/repos/Other/java-monorepo/check-runs/9",
        "https://api.github.com/repos/ChainArgos/other-repo/check-runs/9",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/0",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/09",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/9?x=1",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/9/",
        "https://api.github.com/repos/ChainArgos/java-monorepo/check-runs/not-a-number",
    ];
    let run_body = run(2, SHA);
    for url in invalid_urls {
        let row = job_with_check_run_url(77, url);
        let page = jobs(1, &[row]);
        let mut transport =
            scripted_transport(&[("200", run_body.as_str()), ("200", page.as_str())]);
        assert!(
            block_on_ready(read_actions_workflow_attempt_provider_evidence_async(
                &mut transport,
                OWNER,
                REPOSITORY,
                RUN_ID,
                2,
                SHA,
                TOKEN,
            ))
            .is_err(),
            "invalid check-run URL was accepted: {url}"
        );
        assert_eq!(transport.0.lock().expect("test lock").requests.len(), 2);
    }
}
