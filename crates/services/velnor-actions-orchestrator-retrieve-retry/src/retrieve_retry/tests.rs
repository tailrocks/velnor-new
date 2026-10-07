//! Bounded fetch-retry tests (F5).
//!
//! Declared via `#[path]` from `retrieve_retry.rs` under `cfg(test)`.

use super::*;

#[test]
fn retry_stops_at_first_success_and_at_the_bound() {
    assert_eq!(MAX_DOWNLOAD_ATTEMPTS, 3);
    assert_eq!(download_with_retry(|| true), (true, 1));
    let mut calls = 0u32;
    let (ok, spent) = download_with_retry(|| {
        calls += 1;
        calls >= 2
    });
    assert_eq!((ok, spent, calls), (true, 2, 2));
    let mut calls = 0u32;
    let (ok, spent) = download_with_retry(|| {
        calls += 1;
        false
    });
    assert_eq!(
        (ok, spent, calls),
        (false, MAX_DOWNLOAD_ATTEMPTS, MAX_DOWNLOAD_ATTEMPTS)
    );
}
