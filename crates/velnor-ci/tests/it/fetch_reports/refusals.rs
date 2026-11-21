use super::*;

#[test]
fn the_run_is_listed_once_and_every_expected_archive_is_downloaded_without_leaking_the_token() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("requests", Some(PLAN));
    assert!(fetch(&served, &dir).status.success());
    assert_eq!(served.count(&format!("/repos/{REPO}/actions/runs")), 1);
    assert_eq!(
        served.count(&format!("/repos/{REPO}/actions/artifacts/")),
        31
    );
    let blobs: Vec<_> = served
        .hits()
        .into_iter()
        .filter(|hit| hit.path.starts_with("/blob/"))
        .collect();
    assert_eq!(blobs.len(), 31);
    assert!(blobs.iter().all(|hit| !hit.authorized), "{blobs:?}");
}

#[test]
fn a_report_that_was_never_uploaded_fails_the_run_after_the_others_are_fetched() {
    let mut listing = recorded();
    let name = "velnor-crate-r44491127982-a1-rust-velnor-gen";
    listing["artifacts"] = Value::Array(
        listed(&listing)
            .into_iter()
            .filter(|artifact| artifact["name"] != name)
            .collect(),
    );
    let served = serve(routes(&listing, ARCHIVE));
    let dir = run_dir("missing", Some(PLAN));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(name) && message.contains("not uploaded"),
        "{message}"
    );
    let mut expected = expected_names(PLAN);
    expected.remove(name);
    assert_eq!(fetched(&dir), expected);
}

#[test]
fn a_transient_download_failure_is_retried_and_a_persistent_one_fails_after_three_attempts() {
    let listing = recorded();
    let flaky = id_of(&listing, "velnor-crate-r44491127982-a1-rust-velnor-gen");
    let dead = id_of(&listing, "velnor-crate-r44491127982-a1-rust-velnor-ci");
    let mut table = routes(&listing, ARCHIVE);
    table.insert(
        format!("/blob/{flaky}"),
        vec![Reply::status(500), Reply::ok(ARCHIVE)],
    );
    table.insert(format!("/blob/{dead}"), vec![Reply::status(500)]);
    let served = serve(table);
    let dir = run_dir("retries", Some(PLAN));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(served.count(&format!("/blob/{flaky}")), 2);
    assert_eq!(served.count(&format!("/blob/{dead}")), 3);
    let message = stderr(&output);
    assert!(
        message.contains("rust-velnor-ci") && !message.contains("rust-velnor-gen"),
        "{message}"
    );
    assert!(fetched(&dir).contains("velnor-crate-r44491127982-a1-rust-velnor-gen"));
    assert!(!fetched(&dir).contains("velnor-crate-r44491127982-a1-rust-velnor-ci"));
}

#[test]
fn a_missing_or_malformed_plan_fails_before_any_request() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("plans", None);
    for (label, plan) in [
        ("absent", None),
        ("garbled", Some("{")),
        ("shapeless", Some("{}")),
    ] {
        if let Some(plan) = plan {
            fs::write(dir.join("plan.json"), plan).unwrap_or_else(|e| unreachable!("{e}"));
        }
        assert_eq!(plan.is_some(), dir.join("plan.json").exists());
        let output = fetch(&served, &dir);
        assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
        assert!(
            stderr(&output).contains("plan.json"),
            "{label}: {}",
            stderr(&output)
        );
    }
    assert_eq!(served.count(""), 0);
}

#[test]
fn a_listed_artifact_that_answers_404_is_retried_but_a_410_is_final() {
    let listing = recorded();
    let late = id_of(&listing, "velnor-crate-r44491127982-a1-rust-velnor-gen");
    let gone = id_of(&listing, "velnor-crate-r44491127982-a1-rust-velnor-ci");
    let mut table = routes(&listing, ARCHIVE);
    table.insert(
        format!("/blob/{late}"),
        vec![Reply::status(404), Reply::ok(ARCHIVE)],
    );
    table.insert(format!("/blob/{gone}"), vec![Reply::status(410)]);
    let served = serve(table);
    let dir = run_dir("late-404", Some(PLAN));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(served.count(&format!("/blob/{late}")), 2);
    assert_eq!(served.count(&format!("/blob/{gone}")), 1);
    assert!(fetched(&dir).contains("velnor-crate-r44491127982-a1-rust-velnor-gen"));
    assert!(!fetched(&dir).contains("velnor-crate-r44491127982-a1-rust-velnor-ci"));
}
