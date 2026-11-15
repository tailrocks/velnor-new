// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use super::while_refused;
use crate::error::HostError;

async fn refuse_until(accepted_on: u32, error: HostError) -> (Result<u32, HostError>, u32) {
    let calls = AtomicU32::new(0);
    let outcome = while_refused(Duration::from_secs(30), |_| {
        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
        async move {
            if call >= accepted_on {
                Ok(call)
            } else {
                Err(error)
            }
        }
    })
    .await;
    (outcome, calls.load(Ordering::SeqCst))
}

#[tokio::test]
async fn an_observation_refused_twice_and_then_accepted_passes() {
    assert_eq!(refuse_until(3, HostError::Identity).await, (Ok(3), 3));
}

#[tokio::test]
async fn an_observation_refused_every_time_fails_after_three_attempts() {
    assert_eq!(
        refuse_until(99, HostError::Identity).await,
        (Err(HostError::Identity), 3)
    );
}

#[tokio::test]
async fn an_error_that_is_not_a_refusal_is_not_retried() {
    assert_eq!(
        refuse_until(99, HostError::Docker).await,
        (Err(HostError::Docker), 1)
    );
}

#[tokio::test]
async fn every_attempt_shares_the_one_deadline_so_retries_cannot_extend_it() {
    let seen = Mutex::new(Vec::<Instant>::new());
    let outcome: Result<(), HostError> = while_refused(Duration::from_secs(30), |deadline| {
        if let Ok(mut seen) = seen.lock() {
            seen.push(deadline);
        }
        async { Err(HostError::Identity) }
    })
    .await;
    assert_eq!(outcome, Err(HostError::Identity));
    let seen = seen.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|deadline| *deadline == seen[0]));
}
