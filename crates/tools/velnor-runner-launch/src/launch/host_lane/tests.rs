use super::replay_path;

#[test]
fn ack_refresh_keeps_the_same_message_id_on_the_new_queue_path() {
    assert_eq!(
        replay_path("_apis/runtime/messages", "stale/41", Some("41")),
        "_apis/runtime/messages/41"
    );
}

#[test]
fn acquire_refresh_preserves_its_admin_path() {
    assert_eq!(
        replay_path("_apis/runtime/messages", "scale/3/acquirejobs", None),
        "scale/3/acquirejobs"
    );
}
