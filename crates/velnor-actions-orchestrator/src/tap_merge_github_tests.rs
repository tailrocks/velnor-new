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
