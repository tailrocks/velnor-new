//! Public route matching used by the bounded host transport.

use velnor_runner_github::MessageQueueRoute;

#[test]
fn host_can_validate_the_exact_poll_target_without_reimplementing_query_merge() {
    let route = MessageQueueRoute::from_parts(
        "/messages".to_owned(),
        Some("tenant=private%2Fid&lastMessageId=3&lastMessageId=4".to_owned()),
    )
    .expect("host validated route parts");

    assert!(route.matches_poll_target(
        "/messages",
        Some("tenant=private%2Fid&lastMessageId=3&lastMessageId=4")
    ));
    assert!(route.matches_poll_target("/messages", Some("lastMessageId=9&tenant=private%2Fid")));
    assert!(!route.matches_poll_target(
        "/messages",
        Some("lastMessageId=9&tenant=private%2Fid&extra=1")
    ));
    assert!(!route.matches_poll_target(
        "/messages/other",
        Some("lastMessageId=9&tenant=private%2Fid")
    ));
}

#[test]
fn host_can_validate_the_exact_ack_target_and_preserved_query() {
    let route = MessageQueueRoute::from_parts(
        "/messages".to_owned(),
        Some("tenant=private%2Fid&cursor=opaque".to_owned()),
    )
    .expect("host validated route parts");

    assert!(route.matches_ack_target("/messages/42", Some("tenant=private%2Fid&cursor=opaque")));
    assert!(!route.matches_ack_target("/messages/042", Some("tenant=private%2Fid&cursor=opaque")));
    assert!(!route.matches_ack_target("/messages/42", Some("tenant=private%2Fid&cursor=changed")));
    assert!(!route.matches_ack_target(
        "/messages/42/extra",
        Some("tenant=private%2Fid&cursor=opaque")
    ));
}
