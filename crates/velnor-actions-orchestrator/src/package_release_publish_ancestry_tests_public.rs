use serde_json::json;

use super::super::{verify_public_preview_main_ancestry, verify_public_pull_request_lineage};
use crate::package_release_publish::github::GitHubApi;

#[test]
fn public_only_ancestry_uses_protected_branch_flag_and_rechecks_tip_without_status_token() {
    let source = "a".repeat(40);
    let authority = "b".repeat(40);
    let tip = "c".repeat(40);
    let branch = json!({"name":"main","protected":true,"commit":{"sha":tip}});
    let routes = vec![
        (
            "repos/owner/repo".to_owned(),
            json!({"id":42,"full_name":"owner/repo","default_branch":"main"}),
        ),
        ("repos/owner/repo/branches/main".to_owned(), branch.clone()),
        (
            format!("repos/owner/repo/commits/{source}"),
            json!({"sha":source}),
        ),
        (
            format!("repos/owner/repo/commits/{authority}"),
            json!({"sha":authority}),
        ),
        (
            format!("repos/owner/repo/commits/{tip}"),
            json!({"sha":tip}),
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
        (
            format!("repos/owner/repo/compare/{authority}...{tip}?per_page=1&page=2"),
            json!({
                "base_commit":{"sha":authority},"merge_base_commit":{"sha":authority},
                "status":"ahead","ahead_by":1,"total_commits":1
            }),
        ),
        (
            format!("repos/owner/repo/git/commits/{tip}"),
            json!({"sha":tip,"parents":[{"sha":authority}]}),
        ),
        ("repos/owner/repo/branches/main".to_owned(), branch),
    ];
    let (api, fixture) = GitHubApi::with_fixture("owner/repo", routes);
    assert_eq!(
        verify_public_preview_main_ancestry(&api, 42, "main", &source, &authority)
            .expect("public protected ancestry"),
        tip
    );
    fixture.assert_drained();
}

#[test]
fn public_only_ancestry_rejects_an_unprotected_branch_without_reading_commits() {
    let (api, fixture) = GitHubApi::with_fixture(
        "owner/repo",
        vec![
            (
                "repos/owner/repo".to_owned(),
                json!({"id":42,"full_name":"owner/repo","default_branch":"main"}),
            ),
            (
                "repos/owner/repo/branches/main".to_owned(),
                json!({"name":"main","protected":false,"commit":{"sha":"a".repeat(40)}}),
            ),
        ],
    );
    assert!(
        verify_public_preview_main_ancestry(&api, 42, "main", &"a".repeat(40), &"b".repeat(40))
            .is_err()
    );
    fixture.assert_drained();
}

#[test]
fn public_pr_lineage_requires_protected_current_base_and_descendant_head() {
    let base = "a".repeat(40);
    let head = "b".repeat(40);
    let branch = json!({"name":"main","protected":true,"commit":{"sha":base}});
    let routes = vec![
        (
            "repos/owner/repo".to_owned(),
            json!({"id":42,"full_name":"owner/repo","default_branch":"main"}),
        ),
        ("repos/owner/repo/branches/main".to_owned(), branch.clone()),
        (
            format!("repos/owner/repo/commits/{head}"),
            json!({"sha":head}),
        ),
        (
            format!("repos/owner/repo/compare/{base}...{head}?per_page=1&page=2"),
            json!({
                "base_commit":{"sha":base},"merge_base_commit":{"sha":base},
                "status":"ahead","ahead_by":1,"total_commits":1
            }),
        ),
        (
            format!("repos/owner/repo/git/commits/{head}"),
            json!({"sha":head,"parents":[{"sha":base}]}),
        ),
        ("repos/owner/repo/branches/main".to_owned(), branch),
    ];
    let (api, fixture) = GitHubApi::with_fixture("owner/repo", routes);
    verify_public_pull_request_lineage(&api, 42, "main", &base, &head)
        .expect("exact PR head descends from current protected base");
    fixture.assert_drained();
}

#[test]
fn public_pr_lineage_rejects_unprotected_or_diverged_branches_and_base_tip_changes() {
    let base = "a".repeat(40);
    let head = "b".repeat(40);
    for (protected, changed_tip, compare_status) in [
        (false, false, "ahead"),
        (true, false, "diverged"),
        (true, true, "ahead"),
    ] {
        let initial_branch = json!({
            "name":"main","protected":protected,"commit":{"sha":base}
        });
        let routes = if protected {
            let merge_base = if compare_status == "ahead" {
                base.clone()
            } else {
                "c".repeat(40)
            };
            let mut routes = vec![
                (
                    "repos/owner/repo".to_owned(),
                    json!({"id":42,"full_name":"owner/repo","default_branch":"main"}),
                ),
                ("repos/owner/repo/branches/main".to_owned(), initial_branch),
                (
                    format!("repos/owner/repo/commits/{head}"),
                    json!({"sha":head}),
                ),
                (
                    format!("repos/owner/repo/compare/{base}...{head}?per_page=1&page=2"),
                    json!({
                        "base_commit":{"sha":base},"merge_base_commit":{"sha":merge_base},
                        "status":compare_status,"ahead_by":1,"total_commits":1
                    }),
                ),
            ];
            if compare_status == "ahead" {
                routes.push((
                    format!("repos/owner/repo/git/commits/{head}"),
                    json!({"sha":head,"parents":[{"sha":base}]}),
                ));
            }
            if compare_status == "ahead" {
                routes.push((
                    "repos/owner/repo/branches/main".to_owned(),
                    json!({"name":"main","protected":true,"commit":{"sha":if changed_tip {"d".repeat(40)} else {base.clone()}}}),
                ));
            }
            routes
        } else {
            vec![
                (
                    "repos/owner/repo".to_owned(),
                    json!({"id":42,"full_name":"owner/repo","default_branch":"main"}),
                ),
                ("repos/owner/repo/branches/main".to_owned(), initial_branch),
            ]
        };
        let (api, fixture) = GitHubApi::with_fixture("owner/repo", routes);
        assert!(
            verify_public_pull_request_lineage(&api, 42, "main", &base, &head).is_err(),
            "accepted protected={protected}, changed_tip={changed_tip}, status={compare_status}"
        );
        fixture.assert_drained();
    }
}
