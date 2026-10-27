// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::Value;

struct RouteResponses {
    fixture: GitFixture,
    identity: NativeCheckIdentityExtension,
    selection: crate::package_release_publish::MergeGroupSelection,
    queue: Value,
    pull: Value,
}

fn responses() -> Result<RouteResponses, Box<dyn Error>> {
    let fixture = GitFixture::new()?;
    fixture.checkout(&fixture.queue)?;
    let identity = queue_identity(&fixture);
    let head_ref = format!(
        "refs/heads/gh-readonly-queue/main/pr-515-{}",
        &fixture.base[..8]
    );
    let event = serde_json::json!({
        "action": "checks_requested",
        "merge_group": {"base_sha": fixture.base, "head_sha": fixture.queue,
            "base_ref": "refs/heads/main", "head_ref": head_ref},
        "repository": {"full_name": REPOSITORY, "id": REPOSITORY_ID}
    });
    let selection = crate::homebrew::authority::parse_merge_group_event(
        REPOSITORY,
        REPOSITORY_ID,
        &fixture.queue,
        "merge_group",
        &crate::homebrew::authority::RunnerContext {
            repository: Some(REPOSITORY.to_owned()),
            sha: Some(fixture.queue.clone()),
            git_ref: Some(head_ref),
            run_id: Some("17".to_owned()),
            run_attempt: Some("1".to_owned()),
        },
        &event,
    )?;
    let queue = serde_json::json!({
        "data": {"repository": {"databaseId": REPOSITORY_ID,
            "nameWithOwner": REPOSITORY, "mergeQueue": {"entries": {
                "nodes": [{"baseCommit": {"oid": fixture.base},
                    "headCommit": {"oid": fixture.queue}, "pullRequest": {
                        "number": 515, "baseRefOid": fixture.base,
                        "baseRefName": "main", "headRefOid": fixture.head,
                        "headRefName": "cutover",
                        "baseRepository": {"databaseId": REPOSITORY_ID, "nameWithOwner": REPOSITORY},
                        "headRepository": {"databaseId": REPOSITORY_ID, "nameWithOwner": REPOSITORY}
                    }}], "pageInfo": {"hasNextPage": false}
            }}}}
    });
    let pull = serde_json::json!({
        "number": 515, "state": "open", "draft": false,
        "merged_at": null, "mergeable": true,
        "base": {"ref": "main", "sha": fixture.base,
            "repo": {"id": REPOSITORY_ID, "full_name": REPOSITORY}},
        "head": {"ref": "cutover", "sha": fixture.head,
            "repo": {"id": REPOSITORY_ID, "full_name": REPOSITORY}},
        "user": expected_author()
    });
    Ok(RouteResponses {
        fixture,
        identity,
        selection,
        queue,
        pull,
    })
}

fn verify(responses: &RouteResponses, queue: &Value, pull: &Value) -> Result<(), Box<dyn Error>> {
    let queue = serde_json::to_vec(queue)?;
    let pull = serde_json::to_vec(pull)?;
    Ok(super::super::verify_cutover_author(
        responses.fixture.root(),
        &responses.identity,
        WorkflowEvent::MergeGroup,
        Some(&responses.selection),
        CheckDeadline::after(Duration::from_secs(10)).expect("deadline"),
        |_, _| Ok((queue, pull)),
    )?)
}

#[test]
fn merge_group_cutover_author_accepts_associated_pull_request() -> Result<(), Box<dyn Error>> {
    let responses = responses()?;
    let result = verify(&responses, &responses.queue, &responses.pull);
    assert!(result.is_ok(), "merge-group cutover author: {result:?}");
    Ok(())
}

#[test]
fn merge_group_cutover_author_rejects_stale_or_mismatched_associations()
-> Result<(), Box<dyn Error>> {
    let responses = responses()?;
    let mut wrong_author = responses.pull.clone();
    wrong_author["user"]["login"] = serde_json::json!("untrusted-author");
    assert!(verify(&responses, &responses.queue, &wrong_author).is_err());

    let mut pull_base_drift = responses.pull.clone();
    pull_base_drift["base"]["sha"] = serde_json::json!(responses.fixture.other_base);
    assert!(verify(&responses, &responses.queue, &pull_base_drift).is_err());

    let mut joint_base_drift = responses.queue.clone();
    let new_base = serde_json::json!(responses.fixture.other_base);
    joint_base_drift["data"]["repository"]["mergeQueue"]["entries"]["nodes"][0]["pullRequest"]["baseRefOid"] =
        new_base.clone();
    let mut joint_pull_drift = responses.pull.clone();
    joint_pull_drift["base"]["sha"] = new_base;
    assert!(verify(&responses, &joint_base_drift, &joint_pull_drift).is_err());

    let mut stale = responses.queue.clone();
    stale["data"]["repository"]["mergeQueue"]["entries"]["nodes"][0]["headCommit"]["oid"] =
        serde_json::json!(responses.fixture.unrelated);
    assert!(verify(&responses, &stale, &responses.pull).is_err());

    let mut missing = responses.queue.clone();
    missing["data"]["repository"]["mergeQueue"]["entries"]["nodes"] = serde_json::json!([]);
    assert!(verify(&responses, &missing, &responses.pull).is_err());

    let mut ambiguous = responses.queue.clone();
    let entry = ambiguous["data"]["repository"]["mergeQueue"]["entries"]["nodes"][0].clone();
    ambiguous["data"]["repository"]["mergeQueue"]["entries"]["nodes"] =
        serde_json::json!([entry.clone(), entry]);
    assert!(verify(&responses, &ambiguous, &responses.pull).is_err());

    let mut truncated = responses.queue.clone();
    truncated["data"]["repository"]["mergeQueue"]["entries"]["pageInfo"]["hasNextPage"] =
        serde_json::json!(true);
    assert!(verify(&responses, &truncated, &responses.pull).is_err());

    let mut unrelated_head = responses.queue.clone();
    unrelated_head["data"]["repository"]["mergeQueue"]["entries"]["nodes"][0]["pullRequest"]["headRefOid"] =
        serde_json::json!(responses.fixture.unrelated);
    assert!(verify(&responses, &unrelated_head, &responses.pull).is_err());

    let mut unrelated_pull = responses.pull.clone();
    unrelated_pull["number"] = serde_json::json!(516);
    assert!(verify(&responses, &responses.queue, &unrelated_pull).is_err());
    Ok(())
}
