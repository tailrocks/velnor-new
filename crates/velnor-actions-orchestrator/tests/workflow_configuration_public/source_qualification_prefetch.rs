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

fn warnings(outcome: &Outcome) -> Vec<&str> {
    outcome
        .stdout
        .lines()
        .filter(|line| line.starts_with("::warning::"))
        .collect()
}

const ABSENT: &str = "registry/index=absent registry/cache=absent git/db=absent";

#[test]
fn a_prefix_hit_that_fails_offline_is_discarded_and_fetched_cold() -> Result<(), Box<dyn Error>> {
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
    assert_eq!(
        outcome.field("restored_sources_discarded"),
        Some(ROOT_MANIFEST)
    );
    assert_eq!(warnings(&outcome).len(), 1, "{}", outcome.stdout);
    Ok(())
}

#[test]
fn a_corrupt_restored_entry_is_removed_before_the_network_retry() -> Result<(), Box<dyn Error>> {
    let outcome = run(&scenario(
        "false",
        "velnor-v2-sources-trusted-older",
        &[],
        &[],
    ))?;
    assert_eq!(outcome.status, Some(0), "{}", outcome.stdout);
    assert_eq!(
        outcome.network_sightings,
        [
            format!("{ROOT_MANIFEST} {ABSENT}"),
            format!("{RUNNER_MANIFEST} {ABSENT}"),
        ]
    );
    assert_eq!(
        outcome.field("restored_sources_discarded"),
        Some(ROOT_MANIFEST)
    );
    assert!(
        warnings(&outcome).iter().any(|line| line
            .starts_with("::warning::restored Cargo sources discarded")
            && line.contains(&format!("offline fetch of {ROOT_MANIFEST} failed"))),
        "{}",
        outcome.stdout
    );
    Ok(())
}

#[test]
fn discarding_the_restore_refetches_what_was_already_fetched_offline() -> Result<(), Box<dyn Error>>
{
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
            format!("network {ROOT_MANIFEST}"),
            format!("network {RUNNER_MANIFEST}"),
        ]
    );
    assert_eq!(
        outcome.network_sightings,
        [
            format!("{ROOT_MANIFEST} {ABSENT}"),
            format!("{RUNNER_MANIFEST} {ABSENT}"),
        ]
    );
    assert_eq!(
        outcome.fetch_modes(),
        [
            format!("{ROOT_MANIFEST}:network"),
            format!("{RUNNER_MANIFEST}:network")
        ]
    );
    assert_eq!(
        outcome.field("restored_sources_discarded"),
        Some(RUNNER_MANIFEST)
    );
    assert_eq!(outcome.field("downloaded_crates"), Some("4"));
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
fn an_exact_hit_that_needed_the_network_warns_and_records_the_manifests()
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
            format!("{ROOT_MANIFEST}:network"),
            format!("{RUNNER_MANIFEST}:network")
        ]
    );
    assert_eq!(
        outcome.field("exact_hit_network_fetch"),
        Some(format!("{ROOT_MANIFEST},{RUNNER_MANIFEST}").as_str())
    );
    let exact: Vec<&str> = warnings(&outcome)
        .into_iter()
        .filter(|line| line.starts_with("::warning::exact sources cache hit"))
        .collect();
    assert_eq!(exact.len(), 1, "{}", outcome.stdout);
    assert_eq!(
        exact[0].rsplit_once(": ").map(|(_, manifests)| manifests),
        Some(format!("{ROOT_MANIFEST},{RUNNER_MANIFEST}").as_str())
    );
    Ok(())
}
