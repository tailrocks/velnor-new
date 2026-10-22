use serde_json::json;

use super::{require_ancestor, verify_preview_main_ancestry_with, verify_preview_main_tip_with};
use crate::package_release_publish::github::GitHubApi;

#[test]
fn ancestry_proof_accepts_long_paginated_comparison_and_binds_final_head() {
    let base = "a".repeat(40);
    let head = "b".repeat(40);
    let metadata = json!({
        "base_commit":{"sha":base},
        "merge_base_commit":{"sha":base},
        "status":"ahead",
        "ahead_by":501,
        "total_commits":501,
        "commits":[]
    });
    let final_page = json!({"commits":[{"sha":head}]});
    let (api, fixture) = GitHubApi::with_fixture(
        "owner/repo",
        vec![
            (
                format!("repos/owner/repo/compare/{base}...{head}?per_page=1&page=2"),
                metadata,
            ),
            (
                format!("repos/owner/repo/compare/{base}...{head}?per_page=1&page=501"),
                final_page,
            ),
        ],
    );

    assert!(require_ancestor(&api, &base, &head).is_ok());
    fixture.assert_drained();
}

#[test]
fn ancestry_proof_rejects_diverged_and_misbound_compare_responses() {
    let base = "a".repeat(40);
    let head = "b".repeat(40);
    for (merge_base, status) in [("c".repeat(40), "diverged"), (base.clone(), "ahead")] {
        let response = json!({
            "base_commit":{"sha":base},
            "merge_base_commit":{"sha":merge_base},
            "status":status,
            "ahead_by":2,
            "total_commits":2,
            "commits":[]
        });
        let mut responses = vec![(
            format!("repos/owner/repo/compare/{base}...{head}?per_page=1&page=2"),
            response,
        )];
        if status == "ahead" {
            responses.push((
                format!("repos/owner/repo/compare/{base}...{head}?per_page=1&page=2"),
                json!({"commits":[{"sha":"c".repeat(40)}]}),
            ));
        }
        let (api, fixture) = GitHubApi::with_fixture("owner/repo", responses);
        assert!(require_ancestor(&api, &base, &head).is_err());
        fixture.assert_drained();
    }
}

#[test]
fn one_commit_comparison_checks_the_exact_head_parent() {
    let base = "a".repeat(40);
    let head = "b".repeat(40);
    let (api, fixture) = GitHubApi::with_fixture(
        "owner/repo",
        vec![
            (
                format!("repos/owner/repo/compare/{base}...{head}?per_page=1&page=2"),
                json!({
                    "base_commit":{"sha":base},
                    "merge_base_commit":{"sha":base},
                    "status":"ahead",
                    "ahead_by":1,
                    "total_commits":1
                }),
            ),
            (
                format!("repos/owner/repo/git/commits/{head}"),
                json!({"sha":head,"parents":[{"sha":base}]}),
            ),
        ],
    );
    assert!(require_ancestor(&api, &base, &head).is_ok());
    fixture.assert_drained();
}

#[test]
fn authenticated_ancestry_accepts_source_and_authority_at_unchanged_protected_tip() {
    let source = "a".repeat(40);
    let authority = "b".repeat(40);
    let routes = vec![
        (
            "repos/owner/repo".to_owned(),
            json!({"id":42,"full_name":"owner/repo"}),
        ),
        (
            format!("repos/owner/repo/commits/{source}"),
            json!({"sha":source}),
        ),
        (
            format!("repos/owner/repo/commits/{authority}"),
            json!({"sha":authority}),
        ),
        (
            format!("repos/owner/repo/commits/{authority}"),
            json!({"sha":authority}),
        ),
        (
            format!("repos/owner/repo/compare/{source}...{authority}?per_page=1&page=2"),
            json!({
                "base_commit":{"sha":source},"merge_base_commit":{"sha":source},
                "status":"ahead","ahead_by":1,"total_commits":1
            }),
        ),
        (
            format!("repos/owner/repo/git/commits/{authority}"),
            json!({"sha":authority,"parents":[{"sha":source}]}),
        ),
    ];
    let (api, fixture) = GitHubApi::with_fixture("owner/repo", routes);
    let mut snapshots = [authority.clone(), authority.clone()].into_iter();
    let tip = verify_preview_main_ancestry_with(&api, 42, "main", &source, &authority, || {
        Ok(snapshots.next().expect("two protected-tip snapshots"))
    })
    .expect("source and workflow authority are under the captured protected tip");
    assert_eq!(tip, authority);
    fixture.assert_drained();
}

#[test]
fn authenticated_ancestry_rejects_a_main_ref_change_during_the_proof() {
    let source = "a".repeat(40);
    let authority = "b".repeat(40);
    let routes = vec![
        (
            "repos/owner/repo".to_owned(),
            json!({"id":42,"full_name":"owner/repo"}),
        ),
        (
            format!("repos/owner/repo/commits/{source}"),
            json!({"sha":source}),
        ),
        (
            format!("repos/owner/repo/commits/{authority}"),
            json!({"sha":authority}),
        ),
        (
            format!("repos/owner/repo/commits/{authority}"),
            json!({"sha":authority}),
        ),
        (
            format!("repos/owner/repo/compare/{source}...{authority}?per_page=1&page=2"),
            json!({
                "base_commit":{"sha":source},"merge_base_commit":{"sha":source},
                "status":"ahead","ahead_by":1,"total_commits":1
            }),
        ),
        (
            format!("repos/owner/repo/git/commits/{authority}"),
            json!({"sha":authority,"parents":[{"sha":source}]}),
        ),
    ];
    let (api, fixture) = GitHubApi::with_fixture("owner/repo", routes);
    let mut snapshots = [authority.clone(), "c".repeat(40)].into_iter();
    assert!(
        verify_preview_main_ancestry_with(&api, 42, "main", &source, &authority, || Ok(snapshots
            .next()
            .expect("two protected-tip snapshots")),)
        .is_err()
    );
    fixture.assert_drained();
}

#[test]
fn protected_tip_ref_change_is_an_error_after_authenticated_repository_read() {
    let routes = vec![(
        "repos/owner/repo".to_owned(),
        json!({"id":42,"full_name":"owner/repo"}),
    )];
    let (api, fixture) = GitHubApi::with_fixture("owner/repo", routes);
    let mut tips = ["a".repeat(40), "b".repeat(40)].into_iter();
    let result = verify_preview_main_tip_with(&api, 42, "main", || {
        Ok(tips.next().expect("two protected-tip reads"))
    });
    assert!(result.is_err(), "changed protected main must fail closed");
    fixture.assert_drained();
}

#[test]
fn repository_authentication_failure_is_not_reclassified_as_a_stale_skip() {
    let routes = vec![(
        "repos/owner/repo".to_owned(),
        json!({"id":43,"full_name":"owner/repo"}),
    )];
    let (api, fixture) = GitHubApi::with_fixture("owner/repo", routes);
    let result = verify_preview_main_tip_with(&api, 42, "main", || {
        panic!("unauthenticated repository must not read main")
    });
    assert!(result.is_err());
    fixture.assert_drained();
}
