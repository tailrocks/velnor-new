use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{model, repository};
use crate::error::HostError;

#[test]
fn repository_target_requires_one_owner_and_name() {
    assert_eq!(
        repository::split_repository("acme/runner"),
        Ok(("acme", "runner"))
    );
    assert_eq!(
        repository::split_repository("/runner"),
        Err(crate::scale_set::EnsureError::Rejected)
    );
    assert_eq!(
        repository::split_repository("acme/"),
        Err(crate::scale_set::EnsureError::Rejected)
    );
    assert_eq!(
        repository::split_repository("acme/team/runner"),
        Err(crate::scale_set::EnsureError::Rejected)
    );
}

#[test]
fn binding_fingerprint_is_sha256_lowercase_hex() {
    assert_eq!(
        model::binding_digest(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn remote_deadline_is_capped_by_claim_lease() {
    let requested = Instant::now() + Duration::from_secs(60);
    let lease_until = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_secs(),
    )
    .expect("Unix time fits in i64")
        + 10;
    let deadline = model::deadline_before_lease(requested, lease_until)
        .expect("future lease has a usable deadline");
    assert!(deadline <= requested);
    assert!(deadline > Instant::now());
}

#[test]
fn remote_deadline_preserves_fractional_second_before_lease_expiry() {
    let anchor = Instant::now();
    let wall_clock_now = UNIX_EPOCH + Duration::from_millis(100_900);
    let requested = anchor + Duration::from_secs(60);
    let deadline = model::deadline_before_lease_at(requested, 101, wall_clock_now, anchor)
        .expect("lease has a positive fractional remainder");

    assert_eq!(deadline.duration_since(anchor), Duration::from_millis(100));
}

#[test]
fn expired_claim_lease_cannot_start_remote_observation() {
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_secs(),
    )
    .expect("Unix time fits in i64");
    assert_eq!(
        model::deadline_before_lease(Instant::now() + Duration::from_secs(60), now - 1),
        Err(HostError::Identity)
    );
}
