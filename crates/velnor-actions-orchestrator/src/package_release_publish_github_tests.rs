use super::*;

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
fn annotated_tag_response_exposes_identity_message_and_commit_target() {
    let tag: GitTagResponse = serde_json::from_str(
        r#"{"tag":"v1.2.3-preview.7+abcdef0","message":"signed recovery envelope","object":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","type":"commit"}}"#,
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
        r#"{"tag":"v1.2.3-preview.8+abcdef0","message":"signed recovery envelope","object":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","type":"commit"}}"#,
    )
    .expect("annotated tag response");
    assert!(annotated_tag_observation("v1.2.3-preview.7+abcdef0", tag).is_err());
}
