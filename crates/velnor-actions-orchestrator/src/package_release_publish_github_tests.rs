use super::*;

#[test]
fn graphql_is_sent_through_gh_api_with_standard_headers_and_fields() {
    let query = "query($owner:String!){repository(owner:$owner){id}}";
    let args = gh_api_arguments(
        graphql_arguments(query, &[("owner", "jackin-project".to_owned())]),
        "application/vnd.github+json",
    );
    let args = args
        .iter()
        .map(|arg| arg.to_str().expect("UTF-8 fixture arg"))
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        [
            "api",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "X-GitHub-Api-Version: 2026-03-10",
            "graphql",
            "--raw-field",
            format!("query={query}").as_str(),
            "--field",
            "owner=jackin-project",
        ]
    );
}

#[test]
fn api_path_segments_encode_version_delimiters_and_spaces() {
    assert_eq!(
        encode_segment("v1.2.3-preview.7+abcdef0"),
        "v1.2.3-preview.7%2Babcdef0"
    );
    assert_eq!(encode_segment("branch/name"), "branch%2Fname");
}

#[test]
fn release_api_response_parses_immutable_digest_and_actions_author() {
    let response: ReleaseResponse = serde_json::from_str(
        r#"{"id":9,"tag_name":"v1.2.3-preview.7+abcdef0","draft":true,"prerelease":true,"immutable":false,"body":"recovery","author":{"login":"github-actions[bot]","id":41898282},"assets":[{"id":10,"name":"release.json","size":123,"digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#,
    ).expect("release response");
    assert_eq!(response.immutable, Some(false));
    assert_eq!(
        response.assets[0].digest.as_deref(),
        Some(format!("sha256:{}", "a".repeat(64)).as_str())
    );
    assert_eq!(
        response.author.as_ref().and_then(|author| author.id),
        Some(GITHUB_ACTIONS_BOT_ID)
    );
}

#[test]
fn github_asset_digest_format_is_lowercase_sha256() {
    assert_eq!(
        sha256(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn immutable_status_requires_a_distinct_read_only_credential() {
    assert!(immutable_status_token(None, Some("write"), Some("tap")).is_err());
    assert!(immutable_status_token(Some(""), Some("write"), Some("tap")).is_err());
    assert!(immutable_status_token(Some("write"), Some("write"), Some("tap")).is_err());
    assert!(immutable_status_token(Some("tap"), Some("write"), Some("tap")).is_err());
    assert_eq!(
        immutable_status_token(Some("admin-read"), Some("write"), Some("tap"))
            .expect("separate status credential"),
        "admin-read"
    );
}

#[test]
fn immutable_status_ignores_owner_metadata_but_requires_enabled_true() {
    let response: ImmutableResponse =
        serde_json::from_str(r#"{"enabled":true,"enforced_by_owner":false}"#)
            .expect("documented immutable settings response");
    assert_eq!(response.enabled, Some(true));
    let organization_enforced: ImmutableResponse =
        serde_json::from_str(r#"{"enabled":true,"enforced_by_owner":true}"#)
            .expect("owner enforcement is recorded independently");
    assert_eq!(organization_enforced.enabled, Some(true));
}

#[test]
fn annotated_tag_response_exposes_identity_message_and_commit_target() {
    let tag: GitTagResponse = serde_json::from_str(
        r#"{"sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","tag":"v1.2.3-preview.7+abcdef0","message":"signed recovery envelope","object":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","type":"commit"}}"#,
    )
    .expect("annotated tag response");
    assert_eq!(tag.tag, "v1.2.3-preview.7+abcdef0");
    assert_eq!(tag.message, "signed recovery envelope");
    assert_eq!(tag.object.object_type, "commit");
    assert_eq!(tag.object.sha.len(), 40);
    let observation = annotated_tag_observation("v1.2.3-preview.7+abcdef0", tag)
        .expect("the ref must point at an identically named annotated tag");
    assert_eq!(observation.source_sha.len(), 40);
    assert!(observation.message.is_some());
}

#[test]
fn annotated_tag_internal_name_must_match_its_ref() {
    let tag: GitTagResponse = serde_json::from_str(
        r#"{"sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","tag":"v1.2.3-preview.8+abcdef0","message":"signed recovery envelope","object":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","type":"commit"}}"#,
    )
    .expect("annotated tag response");
    assert!(annotated_tag_observation("v1.2.3-preview.7+abcdef0", tag).is_err());
}

#[test]
fn release_by_tag_reads_one_exact_tag_and_accepts_only_documented_not_found() {
    use serde_json::json;

    let (api, calls) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                format!(
                    "graphql:owner=example&name=app&tagName=preview-{}",
                    "a".repeat(40)
                ),
                json!({"data":{"repository":{"release":{"databaseId":9,"isDraft":false}}}}),
            ),
            (
                "repos/example/app/releases/9".into(),
                json!({
                    "id": 9,
                    "tag_name": format!("preview-{}", "a".repeat(40)),
                    "draft": false,
                    "prerelease": true,
                    "immutable": true,
                    "assets": []
                }),
            ),
        ],
    );
    let found = api
        .release_by_tag(&format!("preview-{}", "a".repeat(40)))
        .expect("exact release lookup")
        .expect("published release");
    assert_eq!(found.id, 9);
    calls.assert_drained();
    assert_eq!(calls.observed().len(), 2);

    let (api, calls) = GitHubApi::with_fixture(
        "example/app",
        vec![(
            "graphql:owner=example&name=app&tagName=preview-missing".into(),
            json!({"data":{"repository":{"release":null}}}),
        )],
    );
    assert!(
        api.release_by_tag("preview-missing")
            .expect("documented 404 is absence")
            .is_none()
    );
    calls.assert_drained();
}

#[test]
fn release_by_tag_rejects_a_response_for_another_tag() {
    use serde_json::json;

    let (api, calls) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                "graphql:owner=example&name=app&tagName=preview-requested".into(),
                json!({"data":{"repository":{"release":{"databaseId":9,"isDraft":false}}}}),
            ),
            (
                "repos/example/app/releases/9".into(),
                json!({
                    "id": 9,
                    "tag_name": "preview-other",
                    "draft": false,
                    "prerelease": true,
                    "immutable": true,
                    "assets": []
                }),
            ),
        ],
    );
    assert!(api.release_by_tag("preview-requested").is_err());
    calls.assert_drained();
}
