use super::*;

#[test]
fn a_recorded_run_yields_the_artifact_set_and_files_the_legacy_fetch_left_for_merge() -> ResultTest
{
    let served = serve(routes(&recorded(), ARCHIVE));
    let dir = run_dir("recorded", Some(PLAN));
    let output = fetch(&served, &dir);
    assert!(output.status.success(), "{output:?}");
    let legacy: Value = serde_json::from_str(FINAL_REPORT)?;
    let downloaded: BTreeSet<String> = legacy["downloaded_artifact_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|id| id.as_str().map(str::to_owned))
        .collect();
    assert_eq!(downloaded.len(), 31);
    assert_eq!(fetched(&dir), downloaded);
    assert_eq!(expected_names(PLAN), downloaded);
    let members = archive_members(ARCHIVE);
    for name in &downloaded {
        assert_eq!(tree(&dir.join("reports").join(name)), members, "{name}");
    }
    Ok(())
}

#[test]
fn an_expired_report_fails_the_run_and_is_named() {
    let mut listing = recorded();
    let name = "velnor-crate-r44491127982-a1-rust-velnor-ci";
    for artifact in listing["artifacts"].as_array_mut().into_iter().flatten() {
        if artifact["name"] == name {
            artifact["expired"] = json!(true);
        }
    }
    let served = serve(routes(&listing, ARCHIVE));
    let dir = run_dir("expired", Some(PLAN));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(name) && message.contains("expired"),
        "{message}"
    );
    assert!(!fetched(&dir).contains(name));
}

#[test]
fn every_page_of_the_listing_is_read() {
    let listing = recorded();
    let all = listed(&listing);
    let (first, rest) = all.split_at(20);
    let mut table = routes(&listing, ARCHIVE);
    let page = |items: &[Value]| json!({"total_count": all.len(), "artifacts": items}).to_string();
    table.insert(listing_path(1), vec![Reply::ok(page(first))]);
    table.insert(listing_path(2), vec![Reply::ok(page(rest))]);
    let served = serve(table);
    let dir = run_dir("pages", Some(PLAN));
    let output = fetch(&served, &dir);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fetched(&dir), expected_names(PLAN));
}

#[test]
fn an_archive_member_that_leaves_the_report_directory_fails_and_writes_nothing() {
    let served = serve(routes(&recorded(), &hostile_archive()));
    let name = "velnor-crate-r44491127982-a1-rust-velnor-gen";
    let plan = json!({"matrix": {"include": [{"artifact_id": name, "matrix_key": "m-0"}]}});
    let dir = run_dir("hostile", Some(&plan.to_string()));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains(name));
    assert!(fetched(&dir).is_empty());
    assert!(!dir.join("escaped.json").exists());
    assert!(!dir.join("reports").join("escaped.json").exists());
}

#[test]
fn an_artifact_name_that_is_not_a_plain_file_name_is_refused() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let plan = json!({"matrix": {"include": [{"artifact_id": "../escape", "matrix_key": "m-0"}]}});
    let dir = run_dir("traversal", Some(&plan.to_string()));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("../escape"));
    assert_eq!(served.count(""), 0);
}

#[test]
fn a_long_generated_artifact_name_is_expected_and_reported_not_uploaded() {
    let served = serve(routes(&recorded(), ARCHIVE));
    let long = format!("velnor-crate-r44491127982-a1-{}", "x".repeat(200));
    let plan = json!({"matrix": {"include": [{"artifact_id": long, "matrix_key": "m-0"}]}});
    let dir = run_dir("long-name", Some(&plan.to_string()));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains("not uploaded") && !message.contains("plain file name"),
        "{message}"
    );

    let too_long = format!("velnor-crate-r44491127982-a1-{}", "x".repeat(300));
    let plan = json!({"matrix": {"include": [{"artifact_id": too_long, "matrix_key": "m-0"}]}});
    let dir = run_dir("too-long-name", Some(&plan.to_string()));
    let output = fetch(&served, &dir);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(stderr(&output).contains("plain file name"));
}
