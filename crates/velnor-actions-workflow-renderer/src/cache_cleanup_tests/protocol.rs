use super::*;

#[test]
fn rest_client_lists_every_exact_ref_before_deleting_ids() {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("The test could not bind the mock API socket.");
    let address = listener
        .local_addr()
        .expect("The test could not read the mock API address.");
    let exchanges = [
        (
            FIRST_PAGE,
            "200 OK",
            r#"{"total_count":2,"actions_caches":[{"id":11,"ref":"refs/pull/17/merge"}]}"#,
        ),
        (
            "GET /repos/tailrocks/velnor-new/actions/caches?ref=refs%2Fpull%2F17%2Fmerge&per_page=100&page=2",
            "200 OK",
            r#"{"total_count":2,"actions_caches":[{"id":12,"ref":"refs/pull/17/merge"}]}"#,
        ),
        (
            "DELETE /repos/tailrocks/velnor-new/actions/caches/11",
            "204 No Content",
            "",
        ),
        (
            "DELETE /repos/tailrocks/velnor-new/actions/caches/12",
            "204 No Content",
            "",
        ),
    ];
    let server = thread::spawn(move || serve(&listener, &exchanges));
    let output = run_client(CLIENT, &format!("http://{address}"));
    let requests = server
        .join()
        .expect("The mock API thread did not complete.");
    assert!(
        output.status.success(),
        "The cleanup client failed. Error output: {:?}",
        output.stderr
    );
    let stdout = String::from_utf8(output.stdout).expect("The client stdout is not UTF-8.");
    assert!(stdout.contains("listed=2 deleted=2 delete_failed=0"));
    assert!(!stdout.contains("test-token"));
    assert_eq!(requests.len(), 4);
    assert!(requests[0].starts_with("GET "));
    assert!(requests[1].starts_with("GET "));
    assert!(requests[2].starts_with("DELETE "));
    assert!(requests[3].starts_with("DELETE "));
}

#[test]
fn client_rejects_a_mismatched_ref_before_any_delete() {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("The test could not bind the mock API socket.");
    let address = listener
        .local_addr()
        .expect("The test could not read the mock API address.");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("The mock API could not accept the list request.");
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream
                .read_exact(&mut byte)
                .expect("The mock API could not read the request.");
            request.push(byte[0]);
        }
        let text = String::from_utf8(request).expect("The request headers are not UTF-8.");
        assert!(text.starts_with("GET "));
        assert!(text.contains("Authorization: Bearer test-token\r\n"));
        mock_response(
            &mut stream,
            "200 OK",
            r#"{"total_count":1,"actions_caches":[{"id":11,"ref":"refs/heads/main"}]}"#,
        );
    });
    let output = run_client(CLIENT, &format!("http://{address}"));
    server
        .join()
        .expect("The mock API thread did not complete.");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cache_ref_mismatch"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("test-token"));
}

#[test]
fn client_does_not_follow_redirects_with_the_token() {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("The test could not bind the mock API socket.");
    let address = listener
        .local_addr()
        .expect("The test could not read the mock API address.");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("The mock API could not accept the list request.");
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream
                .read_exact(&mut byte)
                .expect("The mock API could not read the request.");
            request.push(byte[0]);
        }
        let text = String::from_utf8(request).expect("The request headers are not UTF-8.");
        assert!(text.contains("Authorization: Bearer test-token\r\n"));
        write!(
            stream,
            "HTTP/1.1 302 Found\r\nLocation: https://example.invalid/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .expect("The mock API could not write the redirect response.");
    });
    let output = run_client(CLIENT, &format!("http://{address}"));
    server
        .join()
        .expect("The mock API thread did not complete.");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cache_list_failed"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("test-token"));
}

#[test]
fn invalid_or_duplicate_cache_ids_fail_before_deletion() {
    for (body, reason) in [
        (
            r#"{"total_count":1,"actions_caches":[{"id":0,"ref":"refs/pull/17/merge"}]}"#,
            "invalid_cache_id",
        ),
        (
            r#"{"total_count":1,"actions_caches":[{"id":true,"ref":"refs/pull/17/merge"}]}"#,
            "invalid_cache_id",
        ),
        (
            r#"{"total_count":2,"actions_caches":[{"id":11,"ref":"refs/pull/17/merge"},{"id":11,"ref":"refs/pull/17/merge"}]}"#,
            "invalid_cache_id",
        ),
    ] {
        let (output, requests) = run_single_response("200 OK", body.to_owned());
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(reason));
        assert_eq!(requests, [FIRST_PAGE]);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("test-token"));
    }
}

#[test]
fn client_enforces_cache_count_and_response_body_bounds() {
    let (count_output, count_requests) = run_single_response(
        "200 OK",
        r#"{"total_count":6401,"actions_caches":[]}"#.to_owned(),
    );
    assert!(!count_output.status.success());
    assert!(String::from_utf8_lossy(&count_output.stderr).contains("invalid_cache_list"));
    assert_eq!(count_requests, [FIRST_PAGE]);

    let oversized = "x".repeat(1024 * 1024 + 1);
    let (body_output, body_requests) = run_single_response("200 OK", oversized);
    assert!(!body_output.status.success());
    assert!(String::from_utf8_lossy(&body_output.stderr).contains("cache_list_failed"));
    assert_eq!(body_requests, [FIRST_PAGE]);
}

#[test]
fn delete_failures_are_reported_and_fail_the_workflow() {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("The test could not bind the mock API socket.");
    let address = listener
        .local_addr()
        .expect("The test could not read the mock API address.");
    let exchanges = [
        (
            FIRST_PAGE,
            "200 OK",
            r#"{"total_count":1,"actions_caches":[{"id":11,"ref":"refs/pull/17/merge"}]}"#,
        ),
        (
            "DELETE /repos/tailrocks/velnor-new/actions/caches/11",
            "403 Forbidden",
            "",
        ),
    ];
    let server = thread::spawn(move || serve(&listener, &exchanges));
    let output = run_client(CLIENT, &format!("http://{address}"));
    let requests = server
        .join()
        .expect("The mock API thread did not complete.");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("delete_failed=1"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("cache_unavailable"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cache_unavailable"));
    assert_eq!(requests.len(), 2);
}

#[test]
fn client_stops_after_the_page_limit() {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("The test could not bind the mock API socket.");
    let address = listener
        .local_addr()
        .expect("The test could not read the mock API address.");
    let server = thread::spawn(move || serve_short_pages(&listener));
    let output = run_client(CLIENT, &format!("http://{address}"));
    let requests = server
        .join()
        .expect("The mock API thread did not complete.");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("page_limit_exceeded"));
    assert_eq!(requests.len(), 64);
}

#[test]
fn client_enforces_its_total_response_budget() {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("The test could not bind the mock API socket.");
    let address = listener
        .local_addr()
        .expect("The test could not read the mock API address.");
    let server = thread::spawn(move || serve_large_pages(&listener));
    let output = run_client(CLIENT, &format!("http://{address}"));
    let requests = server
        .join()
        .expect("The mock API thread did not complete.");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("response_budget_exceeded"));
    assert_eq!(requests.len(), 17);
}

#[test]
fn client_stops_before_network_access_after_its_deadline() {
    let script = CLIENT.replace("if __name__ == \"__main__\":\n    main()", "main(0)");
    assert_ne!(script, CLIENT);
    let output = run_client(&script, "https://api.github.com");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("deadline_exceeded"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("test-token"));
}
