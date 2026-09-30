//! P03 identity matrix: generator, relocation, lanes, toolchain, closure.
//!
//! Unregistered: the parent wires this module into `velnor_orchestrator.rs`
//! (P09 pattern). Cases run through the public plan API only.

use velnor_actions_contract::{BaselineStatus, ObligationDecision, Plan};

use super::impl_common::{
    TestResult, config_with_branch, git, git_line, make_repo, plan_for_source_change,
    write_nextest_task,
};
use super::impl_orch_core::{has_warning, manifest_for, plan_value};

/// Typed plan from a plan-response value.
fn typed(value: &serde_json::Value) -> Result<Plan, Box<dyn std::error::Error>> {
    Ok(serde_json::from_value(value["plan"].clone())?)
}

#[test]
fn generator_identity_has_no_zero_digest() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert_eq!(plan.generator.version, env!("CARGO_PKG_VERSION"));
    assert!(!plan.generator.sha256.is_empty());
    assert!(
        !plan.generator.sha256.bytes().all(|b| b == b'0'),
        "zero digest: {}",
        plan.generator.sha256
    );
    assert!(!plan.generator.target.is_empty());
    Ok(())
}

#[test]
fn relocated_checkout_keeps_identities() -> TestResult {
    let (_first_repo, first) = plan_for_source_change()?;
    let (_second_repo, second) = plan_for_source_change()?;
    assert_eq!(first.task_ids, second.task_ids);
    for (left, right) in first.obligations.iter().zip(&second.obligations) {
        assert_eq!(
            (&left.task_id, &left.task_digest),
            (&right.task_id, &right.task_digest)
        );
        assert_eq!(&left.input_digest, &right.input_digest);
    }
    for entry in &first.matrix.include {
        let other = second
            .matrix
            .include
            .iter()
            .find(|candidate| candidate.task_id == entry.task_id)
            .ok_or("relocated entry")?;
        let (left, right) = (
            entry.cache_ids.as_ref().ok_or("ids")?,
            other.cache_ids.as_ref().ok_or("ids")?,
        );
        assert_eq!(left.lane_id, right.lane_id, "{}", entry.task_id);
        assert_eq!(left.toolchain_id, right.toolchain_id, "{}", entry.task_id);
        assert_eq!(
            left.cache_format_id, right.cache_format_id,
            "{}",
            entry.task_id
        );
    }
    Ok(())
}

#[test]
fn lanes_follow_responsibility() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let lanes: Vec<&str> = plan
        .matrix
        .include
        .iter()
        .map(|entry| {
            entry
                .cache_ids
                .as_ref()
                .map_or("", |ids| ids.lane_id.as_str())
        })
        .collect();
    assert!(!lanes.is_empty());
    assert!(lanes.iter().all(|lane| !lane.is_empty()));
    let clippy = plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.task_id.contains("/clippy/"))
        .ok_or("clippy entry")?;
    let test = plan
        .matrix
        .include
        .iter()
        .find(|entry| entry.task_id.contains("/test/") || entry.task_id.contains("/nextest/"))
        .ok_or("test entry")?;
    assert_ne!(
        clippy.cache_ids.as_ref().ok_or("ids")?.lane_id,
        test.cache_ids.as_ref().ok_or("ids")?.lane_id,
        "distinct responsibilities never share a lane (or target dir)"
    );
    Ok(())
}

#[test]
fn changed_source_executes_despite_matching_digests() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let base = git_line(&["rev-parse", "HEAD"], root)?;
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "two"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let seed = typed(&plan_value(root, "pull_request", Some(&base), &head, None)?)?;
    let manifest = manifest_for(&seed, &base, "testmain")?;
    let plan = typed(&plan_value(
        root,
        "pull_request",
        Some(&base),
        &head,
        Some(&manifest),
    )?)?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute),
        "source edits are never baseline-coverable: {:?}",
        plan.obligations
            .iter()
            .map(|ob| &ob.decision)
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn task_cache_stays_disabled_without_qualification() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    for plan in [
        &seed,
        &typed(&plan_value(
            repo.path(),
            "pull_request",
            seed.base.as_deref(),
            &seed.head,
            Some(&manifest_for(
                &seed,
                &seed.base.clone().unwrap_or_default(),
                "testmain",
            )?),
        )?)?,
    ] {
        for entry in &plan.matrix.include {
            let meta = entry.adapter_metadata.as_object().ok_or("metadata")?;
            assert_eq!(
                meta.get("task_cache_enabled"),
                Some(&serde_json::Value::Bool(false))
            );
            assert!(!meta.contains_key("task_cache_key"), "no unqualified key");
        }
        assert!(
            plan.obligations
                .iter()
                .all(|ob| ob.decision != ObligationDecision::ReusedFromTaskCache),
            "plan time never mints reuse"
        );
    }
    Ok(())
}

#[test]
fn known_lockfile_covers_and_nextest_binds_toolchain() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    write_nextest_task(root)?;
    std::fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let seed = typed(&plan_value(root, "pull_request", Some(&head), &head, None)?)?;
    assert!(
        seed.task_ids.iter().any(|id| id.contains("/nextest/")),
        "{:?}",
        seed.task_ids
    );
    let manifest = manifest_for(&seed, &head, "testmain")?;
    let plan = typed(&plan_value(
        root,
        "pull_request",
        Some(&head),
        &head,
        Some(&manifest),
    )?)?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline),
        "known lockfile plus nextest still covers: {:?}",
        plan.warnings
    );
    assert_eq!(plan.baseline.status, BaselineStatus::Used);
    let (_plain_repo, unseeded) = plan_for_source_change()?;
    let nextest_toolchain = seed
        .matrix
        .include
        .iter()
        .find(|entry| entry.task_id.contains("/nextest/"))
        .and_then(|entry| entry.cache_ids.as_ref())
        .map(|ids| ids.toolchain_id.clone())
        .ok_or("nextest toolchain")?;
    assert!(
        unseeded
            .matrix
            .include
            .iter()
            .filter_map(|entry| entry.cache_ids.as_ref())
            .all(|ids| ids.toolchain_id != nextest_toolchain),
        "nextest selection changes the toolchain identity"
    );
    Ok(())
}

#[test]
fn reports_stay_fresh_per_run() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let root = repo.path();
    let head = seed.head.clone();
    let nodiff = typed(&plan_value(root, "pull_request", Some(&head), &head, None)?)?;
    let manifest = manifest_for(&nodiff, &head, "testmain")?;
    let warm = typed(&plan_value(
        root,
        "pull_request",
        Some(&head),
        &head,
        Some(&manifest),
    )?)?;
    assert!(
        warm.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline)
    );
    let mut tampered = manifest_for(&nodiff, &head, "testmain")?;
    tampered["tasks"][0]["input_digest"] =
        serde_json::Value::String(velnor_actions_contract::digest_b3(b"tampered"));
    let cold = plan_value(root, "pull_request", Some(&head), &head, Some(&tampered))?;
    let plan = typed(&cold)?;
    assert!(
        plan.obligations
            .iter()
            .any(|ob| ob.decision == ObligationDecision::Execute)
    );
    assert!(
        has_warning(&cold, "no_entry"),
        "{:?}",
        cold["plan"]["warnings"]
    );
    Ok(())
}
