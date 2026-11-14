// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_local::scenario::{Outcome, distinct, inside, plan, restores, summary, verdict};
use velnor_local::standin::Job;

#[test]
fn the_report_names_each_job_with_its_key_the_entry_it_restored_and_its_scope() {
    let journal = [
        r#"{"writer":"Actions.Results:7:aaa","key":"cargo-x","matched_key":"cargo-x","scope":"refs/heads/main"}"#,
        r#"{"writer":"Actions.Results:7:bbb","key":"mise-y","matched_key":null,"scope":null}"#,
        r#"{"writer":"Actions.Results:7:bbb","key":"cargo-abc","matched_key":"cargo-","scope":"refs/heads/main"}"#,
        r#"{"writer":"Actions.Results:7:old","key":"stale","matched_key":"stale","scope":"refs/heads/main"}"#,
        r#"{"writer":"Actions.Results:8:aaa","key":"other-run","matched_key":"other-run","scope":"refs/heads/main"}"#,
        "not json",
    ]
    .join("\n");
    let jobs: Vec<Job> = serde_json::from_value(serde_json::json!([
        {"requestId": 1, "jobId": "aaa", "timeLineId": null, "name": "lint", "workflowname": "ci", "result": "succeeded", "attempt": 1, "runid": 7},
        {"requestId": 2, "jobId": "bbb", "timeLineId": null, "name": "test", "workflowname": "ci", "result": "succeeded", "attempt": 1, "runid": 7},
    ]))
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        restores(&journal, 7, &jobs),
        [
            "job lint: key cargo-x restored cargo-x from refs/heads/main",
            "job test: key mise-y found no entry",
            "job test: key cargo-abc restored cargo- from refs/heads/main",
        ]
    );
}

#[test]
fn each_scenario_gets_its_own_environment_and_a_shared_one_is_refused() {
    let names = |list: &[&str]| {
        list.iter()
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>()
    };
    let root = Path::new("/state");
    let one = plan(&names(&["warm"]), root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        one,
        [("warm".to_owned(), "warm".to_owned(), root.to_owned())]
    );
    let two = plan(&names(&["docs", "x/leaf.json"]), root).unwrap_or_else(|e| unreachable!("{e}"));
    let dirs: Vec<&Path> = two.iter().map(|(_, _, dir)| dir.as_path()).collect();
    assert_eq!(dirs, [Path::new("/state/docs"), Path::new("/state/leaf")]);
    assert!(distinct(&two).is_ok());
    let dotted = plan(
        &names(&["release", "release.v2", "x/release.v3.json"]),
        root,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let stems: Vec<&str> = dotted.iter().map(|(_, stem, _)| stem.as_str()).collect();
    assert_eq!(stems, ["release", "release.v2", "release.v3"]);
    let resolved = |dir: &str| ("a".to_owned(), "a".to_owned(), PathBuf::from(dir));
    let mut same = vec![resolved("/state/common"), resolved("/state/common")];
    same[1].0 = "b".to_owned();
    assert!(distinct(&same).is_err_and(|error| error.contains("a and b")));
    for shared in [
        &["docs", "docs"][..],
        &["docs", "other/docs.json"],
        &["docs", "docs.json"],
    ] {
        assert!(plan(&names(shared), root).is_err(), "{shared:?}");
    }
    for escaping in [
        &["docs", ".."][..],
        &["docs", "/"],
        &["docs", "/tmp/...json"],
        &["docs", "x/..json"],
    ] {
        assert!(plan(&names(escaping), root).is_err(), "{escaping:?}");
    }
}

#[test]
fn a_scenario_directory_that_resolves_outside_the_state_directory_is_refused() {
    let root = Path::new("/state");
    assert!(inside(root, root));
    assert!(inside(root, Path::new("/state/docs")));
    assert!(!inside(root, Path::new("/elsewhere/docs")));
    assert!(!inside(root, Path::new("/")));
    assert!(!inside(root, Path::new("/statement/docs")));
}

#[test]
fn one_failed_scenario_is_named_and_does_not_hide_the_others() {
    let results = [
        ("docs".to_owned(), Ok(())),
        ("leaf".to_owned(), Err("a run did not succeed".to_owned())),
        ("hub".to_owned(), Ok(())),
    ];
    assert_eq!(
        summary(&results),
        Err("leaf: a run did not succeed".to_owned())
    );
    assert_eq!(summary(&results[..1]), Ok(()));
}

#[test]
fn every_verdict_line_names_its_scenario_and_carries_the_cache_lines() {
    let outcome = Outcome {
        id: 3,
        workflow: "ci".to_owned(),
        event: "push".to_owned(),
        conclusion: "failure",
        started: Some(110),
        metrics: Err("no".to_owned()),
        caches: vec!["job lint: key k found no entry".to_owned()],
    };
    let text = verdict("leaf", &[outcome], 100, 4.5);
    assert!(
        text.lines()
            .all(|line| line.starts_with("velnor-local leaf: ")),
        "{text}"
    );
    assert!(
        text.contains("velnor-local leaf:     job lint: key k found no entry"),
        "{text}"
    );
    assert!(
        text.contains("first job started 10 s after up began"),
        "{text}"
    );
}
