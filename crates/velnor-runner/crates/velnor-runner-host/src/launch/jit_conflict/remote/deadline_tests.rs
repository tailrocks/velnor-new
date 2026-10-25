use super::*;

#[test]
fn operation_deadline_is_capped_and_stays_inside_the_claim_lease() {
    let start = Instant::now();
    assert_eq!(
        operation_deadline_at(1_000, 500, start).expect("capped deadline"),
        start + Duration::from_secs(OPERATION_CAP_SECONDS)
    );
    assert_eq!(
        operation_deadline_at(525, 500, start).expect("lease-bounded deadline"),
        start + Duration::from_secs(10)
    );
    assert!(operation_deadline_at(515, 500, start).is_err());
    assert!(operation_deadline_at(500, 500, start).is_err());
}

#[test]
fn admin_origin_stays_pinned_when_the_poll_link_moves_to_a_queue_host() {
    let mut link = crate::listen::inert_link_for_offline_test();
    let admin_origin = link.admin_origin().to_owned();

    crate::listen::point_at_queue(&mut link, "https://queue.example/_apis/message")
        .expect("queue route is valid");

    assert_ne!(link.base(), admin_origin);
    assert_eq!(link.admin_origin(), admin_origin);
}
