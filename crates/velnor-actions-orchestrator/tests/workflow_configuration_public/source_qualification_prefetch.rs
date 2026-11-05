// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;

use super::source_qualification_prefetch_harness::{
    Outcome, Scenario, has_flag, lock_workspace, run_prefetch,
};

const ROOT_MANIFEST: &str = "Cargo.toml";
const RUNNER_MANIFEST: &str = "crates/velnor-runner/Cargo.toml";

fn assert_every_call_is_locked(outcome: &Outcome) {
    assert!(!outcome.calls.is_empty(), "the stub recorded no cargo call");
    for call in &outcome.calls {
        assert!(has_flag(call, "--locked"), "call without --locked: {call}");
        assert!(has_flag(call, "fetch"), "not a cargo fetch: {call}");
    }
}

fn scenario(
    cache_hit: &'static str,
    matched_key: &'static str,
    offline_ok: &'static [&'static str],
    network_fails: &'static [&'static str],
) -> Scenario {
    Scenario {
        cache_hit,
        matched_key,
        offline_ok,
        network_fails,
    }
}

fn run(scenario: &Scenario) -> Result<Outcome, Box<dyn Error>> {
    let workspace = lock_workspace()?;
    run_prefetch(workspace.path(), scenario)
}

#[test]
fn an_exact_hit_fetches_offline_only_and_downloads_nothing() -> Result<(), Box<dyn Error>> {
    let outcome = run(&scenario(
        "true",
        "velnor-v2-sources-trusted-k",
        &[ROOT_MANIFEST, RUNNER_MANIFEST],
        &[],
    ))?;
    assert_eq!(outcome.status, Some(0), "{}", outcome.stdout);
    assert_every_call_is_locked(&outcome);
    assert_eq!(
        outcome.sequence(),
        [
            format!("offline {ROOT_MANIFEST}"),
            format!("offline {RUNNER_MANIFEST}")
        ]
    );
    assert_eq!(outcome.network_calls, 0);
    assert_eq!(
        outcome.fetch_modes(),
        [
            format!("{ROOT_MANIFEST}:offline"),
            format!("{RUNNER_MANIFEST}:offline")
        ]
    );
    assert_eq!(outcome.field("downloaded_crates"), Some("0"));
    assert_eq!(outcome.field("exact_hit_network_fetch"), None);
    assert!(
        !outcome.stdout.contains("::warning::"),
        "{}",
        outcome.stdout
    );
    Ok(())
}

#[test]
fn a_miss_fetches_from_the_network_without_trying_offline() -> Result<(), Box<dyn Error>> {
    let outcome = run(&scenario("", "", &[], &[]))?;
    assert_eq!(outcome.status, Some(0), "{}", outcome.stdout);
    assert_every_call_is_locked(&outcome);
    assert_eq!(
        outcome.sequence(),
        [
            format!("network {ROOT_MANIFEST}"),
            format!("network {RUNNER_MANIFEST}")
        ]
    );
    assert_eq!(outcome.network_calls, 2);
    assert_eq!(
        outcome.fetch_modes(),
        [
            format!("{ROOT_MANIFEST}:network"),
            format!("{RUNNER_MANIFEST}:network")
        ]
    );
    assert_eq!(outcome.field("downloaded_crates"), Some("4"));
    assert_eq!(outcome.field("sources_cache_matched_key"), Some(""));
    assert_eq!(outcome.field("exact_hit_network_fetch"), None);
    assert!(
        !outcome.stdout.contains("::warning::"),
        "{}",
        outcome.stdout
    );
    Ok(())
}

#[test]
fn a_prefix_hit_tries_offline_then_falls_back_to_the_network() -> Result<(), Box<dyn Error>> {
    let outcome = run(&scenario(
        "false",
        "velnor-v2-sources-trusted-older",
        &[],
        &[],
    ))?;
    assert_eq!(outcome.status, Some(0), "{}", outcome.stdout);
    assert_every_call_is_locked(&outcome);
    assert_eq!(
        outcome.sequence(),
        [
            format!("offline {ROOT_MANIFEST}"),
            format!("network {ROOT_MANIFEST}"),
            format!("offline {RUNNER_MANIFEST}"),
            format!("network {RUNNER_MANIFEST}"),
        ]
    );
    assert_eq!(outcome.network_calls, 2);
    assert_eq!(
        outcome.fetch_modes(),
        [
            format!("{ROOT_MANIFEST}:network"),
            format!("{RUNNER_MANIFEST}:network")
        ]
    );
    assert_eq!(outcome.field("downloaded_crates"), Some("4"));
    assert_eq!(outcome.field("exact_hit_network_fetch"), None);
    assert!(
        !outcome.stdout.contains("::warning::"),
        "{}",
        outcome.stdout
    );
    Ok(())
}

#[test]
fn a_mixed_restore_fetches_each_manifest_by_its_own_outcome() -> Result<(), Box<dyn Error>> {
    let outcome = run(&scenario(
        "false",
        "velnor-v2-sources-trusted-older",
        &[ROOT_MANIFEST],
        &[],
    ))?;
    assert_eq!(outcome.status, Some(0), "{}", outcome.stdout);
    assert_every_call_is_locked(&outcome);
    assert_eq!(
        outcome.sequence(),
        [
            format!("offline {ROOT_MANIFEST}"),
            format!("offline {RUNNER_MANIFEST}"),
            format!("network {RUNNER_MANIFEST}"),
        ]
    );
    assert_eq!(outcome.network_calls, 1);
    assert_eq!(
        outcome.fetch_modes(),
        [
            format!("{ROOT_MANIFEST}:offline"),
            format!("{RUNNER_MANIFEST}:network")
        ]
    );
    assert_eq!(outcome.field("downloaded_crates"), Some("2"));
    Ok(())
}

#[test]
fn a_manifest_that_fails_offline_and_online_fails_the_phase_and_is_recorded()
-> Result<(), Box<dyn Error>> {
    let outcome = run(&scenario(
        "true",
        "velnor-v2-sources-trusted-k",
        &[],
        &[ROOT_MANIFEST, RUNNER_MANIFEST],
    ))?;
    assert_eq!(outcome.status, Some(1), "{}", outcome.stdout);
    assert_every_call_is_locked(&outcome);
    assert_eq!(
        outcome.fetch_modes(),
        [
            format!("{ROOT_MANIFEST}:failed"),
            format!("{RUNNER_MANIFEST}:failed")
        ]
    );
    assert_eq!(outcome.network_calls, 0);
    assert_eq!(outcome.field("downloaded_crates"), Some("0"));
    Ok(())
}

#[test]
fn an_exact_hit_that_needed_the_network_warns_and_records_the_manifest()
-> Result<(), Box<dyn Error>> {
    let outcome = run(&scenario(
        "true",
        "velnor-v2-sources-trusted-k",
        &[ROOT_MANIFEST],
        &[],
    ))?;
    assert_eq!(outcome.status, Some(0), "{}", outcome.stdout);
    assert_every_call_is_locked(&outcome);
    assert_eq!(
        outcome.fetch_modes(),
        [
            format!("{ROOT_MANIFEST}:offline"),
            format!("{RUNNER_MANIFEST}:network")
        ]
    );
    assert_eq!(
        outcome.field("exact_hit_network_fetch"),
        Some(RUNNER_MANIFEST)
    );
    let warnings: Vec<&str> = outcome
        .stdout
        .lines()
        .filter(|line| line.starts_with("::warning::"))
        .collect();
    assert_eq!(warnings.len(), 1, "{}", outcome.stdout);
    assert_eq!(
        warnings[0]
            .rsplit_once(": ")
            .map(|(_, manifests)| manifests),
        Some(RUNNER_MANIFEST)
    );
    Ok(())
}
