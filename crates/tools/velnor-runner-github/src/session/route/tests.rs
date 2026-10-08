use super::MessageQueueRoute;

#[test]
fn poll_query_preserves_and_canonicalizes_values_like_url_values_encode() {
    let route = MessageQueueRoute::from_parts(
        "/messages".to_owned(),
        Some("z=two+words&lastMessageId=3&a=%2B&lastMessageId=4".to_owned()),
    )
    .expect("validated route");
    assert_eq!(
        route.poll_query(9).as_deref(),
        Some("a=%2B&lastMessageId=9&z=two+words")
    );
    assert_eq!(
        route.poll_query(0).as_deref(),
        Some("z=two+words&lastMessageId=3&a=%2B&lastMessageId=4")
    );
}

#[test]
fn acknowledgement_appends_path_and_keeps_the_query_separate() {
    let route =
        MessageQueueRoute::from_parts("/messages".to_owned(), Some("cursor=opaque".to_owned()))
            .expect("validated route");
    assert_eq!(route.acknowledgement_path(17), "/messages/17");
    assert_eq!(route.query(), Some("cursor=opaque"));
}

#[test]
fn route_debug_does_not_include_path_or_query() {
    let route = MessageQueueRoute::from_parts(
        "/secret/path".to_owned(),
        Some("routing-secret=hidden".to_owned()),
    )
    .expect("validated route");
    let debug = format!("{route:?}");
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("hidden"));
}

#[test]
fn route_rejects_bad_escapes_and_traversal() {
    for path in ["/messages/%", "/messages/%2fother", "/messages/../other"] {
        assert!(MessageQueueRoute::from_parts(path.to_owned(), None).is_err());
    }
    assert!(
        MessageQueueRoute::from_parts("/messages".to_owned(), Some("bad=%Q0".to_owned())).is_err()
    );
}
