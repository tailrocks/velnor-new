use super::*;

#[test]
fn parses_included_rest_status_without_trusting_body_claims() {
    let response = parse_included_response(
        b"HTTP/2.0 200 OK\r\ncontent-type: application/json\r\n\r\n{\"merged\":true}",
    )
    .expect("response");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, br#"{"merged":true}"#);
}

#[test]
fn parses_error_status_for_typed_optional_reads() {
    let response = parse_included_response(b"HTTP/2 404 Not Found\r\n\r\n{}").expect("response");
    assert_eq!(response.status, 404);
}

#[test]
fn rejects_missing_status_and_malformed_headers() {
    assert!(parse_included_response(b"{\"ok\":true}").is_err());
    assert!(parse_included_response(b"HTTP/2 nope\r\n\r\n{}").is_err());
}

#[test]
fn test_transport_intercepts_private_client_requests_without_gh() {
    let api = TapMergeGitHub::with_test_transport(|args| {
        assert_eq!(args, [OsString::from("repos/owner/tap/pulls?state=all")]);
        Ok(ApiResponse {
            status: 200,
            body: br#"[{"number":7}]"#.to_vec(),
        })
    });
    let response: Vec<serde_json::Value> = api
        .get("repos/owner/tap/pulls?state=all")
        .expect("scripted response");
    assert_eq!(response[0]["number"].as_u64(), Some(7));
}

#[test]
fn production_wire_metadata_reads_revalidate_each_page() {
    let route = "repos/owner/tap/pulls/12/reviews?per_page=100&page=2";
    let first = super::request_arguments(vec![route.into()]).expect("first");
    let second = super::request_arguments(vec![route.into()]).expect("second");
    assert_eq!(first[0], "api");
    assert!(
        first
            .iter()
            .any(|arg| arg == "Accept: application/vnd.github+json")
    );
    assert!(
        first
            .iter()
            .any(|arg| arg == "Cache-Control: no-cache, no-store")
    );
    let urls = [&first, &second].map(|args| {
        args.iter()
            .find(|arg| arg.to_str().is_some_and(|value| value.starts_with(route)))
            .expect("paged route")
    });
    assert_ne!(urls[0], urls[1]);
    assert!(
        urls[0]
            .to_str()
            .expect("URL")
            .starts_with(&format!("{route}&_velnor_read_nonce="))
    );
}

#[test]
fn production_wire_merge_keeps_sha_condition_and_mutation_route() {
    let raw = [
        "repos/owner/tap/pulls/12/merge",
        "--method",
        "PUT",
        "--raw-field",
        "sha=validated-head",
    ];
    let args = super::request_arguments(raw.iter().map(std::ffi::OsString::from).collect())
        .expect("merge");
    assert!(args.ends_with(&raw.map(std::ffi::OsString::from)));
    assert!(
        !args.iter().any(|arg| arg.to_str().is_some_and(|value| value
            .contains("_velnor_read_nonce")
            || value.contains("no-cache")))
    );
}
