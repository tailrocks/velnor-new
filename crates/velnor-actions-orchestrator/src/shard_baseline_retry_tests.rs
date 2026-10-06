//! Publication retries preserve proof identity through normal lookup and coverage.

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for, verdict};
use velnor_actions_contract::{canonical_json_bytes, digest_b3};

fn retry_parent() -> BaselineManifest {
    let mut parent = manifest_for(&"1".repeat(40));
    parent.repository_id = digest_b3(b"github.com/o/r");
    parent
}

/// Lookup outcome plus the text and archive transports it exercised.
type RetryLookupOutcome = (
    Result<Vec<BaselineManifest>, String>,
    Vec<Vec<String>>,
    Vec<Vec<String>>,
);

fn normal_retry_lookup(
    parent: &BaselineManifest,
    current_conclusion: &str,
    original_conclusion: &str,
) -> RetryLookupOutcome {
    let lookup = BaselineLookup::new(
        &parent.source_commit,
        ".github/workflows/ci.yml",
        "testmain",
        "o/r",
    )
    .expect("lookup");
    let manifest = canonical_json_bytes(parent).expect("canonical original bytes");
    let archive = crate::baseline_archive::test_archive("baseline.json", &manifest);
    let listed = serde_json::json!({"total_count":1,"artifacts":[{
        "id":parent.artifact_id,"name":parent.artifact_name,"expired":false,
        "size_in_bytes":archive.len(),"digest":format!(
            "sha256:{}",crate::cover_identity::generator::sha256_hex(&archive)
        )
    }]})
    .to_string();
    let mut calls = Vec::new();
    let mut archive_calls = Vec::new();
    let found = resolve_with(
        &lookup,
        &parent.artifact_name,
        |args| {
            let args: Vec<_> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            calls.push(args.clone());
            if args[0] == "run" && args[1] == "list" {
                Ok(
                    serde_json::json!([{"databaseId":7,"headSha":parent.source_commit,
                "event":"push","conclusion":current_conclusion,"headBranch":"testmain",
                "attempt":2}])
                    .to_string(),
                )
            } else if args[1].contains("/artifacts?name=") {
                Ok(listed.clone())
            } else {
                assert!(args[1].ends_with("/runs/7/attempts/1"));
                Ok(
                    serde_json::json!({"id":7,"run_attempt":1,"head_sha":parent.source_commit,
                "event":"push","status":"completed","conclusion":original_conclusion,
                "head_branch":"testmain","path":".github/workflows/ci.yml",
                "repository":{"full_name":"O/R"}})
                    .to_string(),
                )
            }
        },
        |args| {
            archive_calls.push(
                args.iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
            );
            Ok(archive.clone())
        },
    );
    (found, calls, archive_calls)
}

#[test]
fn replayed_publication_remains_usable_for_next_plan_coverage() {
    let retained = retry_parent();
    let original_bytes = canonical_json_bytes(&retained).expect("original bytes");
    // The T16 publisher retains this immutable attempt-1 artifact when
    // attempt 2 succeeds. Normal lookup sees the latest summary attempt 2.
    let (found, calls, archive_calls) = normal_retry_lookup(&retained, "success", "success");
    let found = found
        .expect("normal lookup")
        .pop()
        .expect("retained baseline");
    assert_eq!(found.run_attempt, 1);
    assert_eq!(
        canonical_json_bytes(&found).expect("retrieved bytes"),
        original_bytes
    );
    assert!(
        calls
            .iter()
            .any(|args| args[1].ends_with("/runs/7/attempts/1"))
    );
    assert_eq!(
        archive_calls,
        [vec![
            "api".to_owned(),
            format!("repos/o/r/actions/artifacts/{}/zip", retained.artifact_id)
        ]]
    );
    let next_plan = plan_for(&found, Some(&found.source_commit));
    assert!(crate::covered_tasks::plan_has_covered(&next_plan));
    let (signals, missing) = verdict(&next_plan, Some(&found));
    assert!(!signals.planning_failed, "{missing:?}");
    assert!(missing.is_empty());
}

#[test]
fn current_summary_never_authorizes_original_attempt_success() {
    let parent = retry_parent();
    // Failed retry does not invalidate independently proven original success.
    assert!(normal_retry_lookup(&parent, "failure", "success").0.is_ok());
    // A successful summary cannot turn the downloaded failed attempt into proof.
    assert!(
        normal_retry_lookup(&parent, "success", "failure")
            .0
            .is_err()
    );
    assert!(
        normal_retry_lookup(&parent, "failure", "failure")
            .0
            .is_err()
    );
}
