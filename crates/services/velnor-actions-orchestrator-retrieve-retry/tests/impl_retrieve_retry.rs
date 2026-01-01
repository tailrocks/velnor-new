//! Retry bound: attempts spent, calls made, and the hard stop.
use velnor_actions_orchestrator_retrieve_retry::retrieve_retry::{
    MAX_DOWNLOAD_ATTEMPTS, download_with_retry,
};

#[test]
fn max_attempts_is_three() {
    assert_eq!(MAX_DOWNLOAD_ATTEMPTS, 3);
}

#[test]
fn first_try_success_costs_one() {
    assert_eq!(download_with_retry(|| true), (true, 1));
}

#[test]
fn second_try_success_costs_two() {
    let mut calls = 0u32;
    let (ok, spent) = download_with_retry(|| {
        calls += 1;
        calls >= 2
    });
    assert_eq!((ok, spent, calls), (true, 2, 2));
}

#[test]
fn third_try_success_is_last_chance() {
    let mut calls = 0u32;
    let (ok, spent) = download_with_retry(|| {
        calls += 1;
        calls >= 3
    });
    assert_eq!((ok, spent, calls), (true, 3, 3));
}

#[test]
fn fourth_try_success_still_fails() {
    let mut calls = 0u32;
    let (ok, spent) = download_with_retry(|| {
        calls += 1;
        calls >= 4
    });
    assert_eq!((ok, spent, calls), (false, 3, 3));
}

#[test]
fn persistent_failure_spends_full_budget() {
    assert_eq!(download_with_retry(|| false), (false, 3));
}

#[test]
fn persistent_failure_calls_thrice() {
    let mut calls = 0u32;
    let (ok, spent) = download_with_retry(|| {
        calls += 1;
        false
    });
    assert_eq!((ok, spent, calls), (false, 3, 3));
}

#[test]
fn attempts_never_exceed_max() {
    let mut calls = 0u32;
    let (ok, spent) = download_with_retry(|| {
        calls += 1;
        calls > 100
    });
    assert_eq!((ok, spent), (false, MAX_DOWNLOAD_ATTEMPTS));
    assert_eq!(calls, MAX_DOWNLOAD_ATTEMPTS);
}

#[test]
fn success_stops_further_calls() {
    let mut calls = 0u32;
    let (ok, _) = download_with_retry(|| {
        calls += 1;
        true
    });
    assert!(ok);
    assert_eq!(calls, 1);
}

#[test]
fn late_success_within_budget_wins() {
    let results = [false, false, true];
    let mut index = 0usize;
    let (ok, spent) = download_with_retry(|| {
        let value = results[index.min(2)];
        index += 1;
        value
    });
    assert_eq!((ok, spent), (true, 3));
}
