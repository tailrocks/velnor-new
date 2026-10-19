use std::{cell::Cell, time::Duration};

use crate::error::HostError;

use super::{RequestWaitError, wait_for_request};

#[tokio::test]
async fn request_wait_retries_absence_until_the_hook_publishes() {
    let attempts = Cell::new(0);
    let result = wait_for_request(
        std::time::Instant::now() + Duration::from_secs(1),
        || async {
            let attempt = attempts.get();
            attempts.set(attempt + 1);
            Ok((attempt >= 2).then_some("request"))
        },
    )
    .await;

    assert_eq!(result, Ok("request"));
    assert_eq!(attempts.get(), 3);
}

#[tokio::test]
async fn permanent_absence_stops_at_the_absolute_deadline() {
    let attempts = Cell::new(0);
    let result = wait_for_request(
        std::time::Instant::now() + Duration::from_millis(800),
        || async {
            attempts.set(attempts.get() + 1);
            Ok::<Option<()>, HostError>(None)
        },
    )
    .await;

    assert_eq!(result, Err(RequestWaitError::DeadlineElapsed));
    assert!(attempts.get() > 1);
}

#[tokio::test]
async fn expired_deadline_does_not_start_another_read() {
    let attempts = Cell::new(0);
    let result = wait_for_request(std::time::Instant::now(), || async {
        attempts.set(attempts.get() + 1);
        Ok::<Option<()>, HostError>(None)
    })
    .await;

    assert_eq!(result, Err(RequestWaitError::DeadlineElapsed));
    assert_eq!(attempts.get(), 0);
}

#[tokio::test]
async fn unsafe_or_changed_channel_errors_are_not_retried_as_absence() {
    let attempts = Cell::new(0);
    let result = wait_for_request(
        std::time::Instant::now() + Duration::from_secs(1),
        || async {
            attempts.set(attempts.get() + 1);
            Err::<Option<()>, _>(HostError::Identity)
        },
    )
    .await;

    assert_eq!(result, Err(RequestWaitError::Read(HostError::Identity)));
    assert_eq!(attempts.get(), 1);
}
