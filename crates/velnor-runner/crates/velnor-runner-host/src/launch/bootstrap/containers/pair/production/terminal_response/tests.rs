use super::terminal_create_response_status;

#[test]
fn only_completed_daemon_rejections_receive_a_terminal_receipt() {
    let version = bollard::API_DEFAULT_VERSION;
    assert_eq!((version.major_version, version.minor_version), (1, 53));
    for status in [400, 404, 409] {
        assert!(terminal_create_response_status(status));
    }
    for status in [408, 429, 500, 502, 503] {
        assert!(!terminal_create_response_status(status));
    }
}
