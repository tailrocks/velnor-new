//! P04 reuse/provenance matrix: every unsafe case executes or misses.
//!
//! Unregistered: the parent wires this module into `velnor_orchestrator.rs`
//! (P09 pattern). Cases run through the public plan API only.

use velnor_actions_contract::digest_b3;
use velnor_actions_contract_workflow::{BaselineStatus, ObligationDecision, Plan};

use crate::cases::orch_core::{has_warning, manifest_for, plan_value};
use crate::support::{
    TestResult, config_with_branch, git, git_line, make_repo, plan_for_source_change,
    write_nextest_task,
};

/// Anchor test repos to a fixed origin so provenance can validate.
///
/// Fail-closed anchoring refuses coverage for origin-less checkouts; every
/// unsafe-case test below must anchor first so the specific miss reason —
/// not `repository_unanchored` — is what the test exercises.
fn anchor_repo(root: &std::path::Path) -> TestResult {
    git(
        &["remote", "add", "origin", "https://github.com/o/r.git"],
        root,
    )
}

/// Manifest `repository_id` matching [`anchor_repo`].
fn anchor_id() -> String {
    digest_b3(b"github.com/o/r")
}

/// Typed plan from a plan-response value.
fn typed(value: &serde_json::Value) -> Result<Plan, Box<dyn std::error::Error>> {
    Ok(serde_json::from_value(value["plan"].clone())?)
}

/// Plan with `mutate` applied to an otherwise valid echo manifest.
fn plan_with_mutation(
    root: &std::path::Path,
    seed: &Plan,
    base: &str,
    mutate: &dyn Fn(&mut serde_json::Value),
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut manifest = manifest_for(seed, base, "testmain")?;
    manifest["repository_id"] = serde_json::Value::String(anchor_id());
    mutate(&mut manifest);
    plan_value(
        root,
        "pull_request",
        Some(base),
        &seed.head,
        Some(&manifest),
    )
}

/// Every mutation executes everything with its precise miss reason.
fn assert_miss(value: &serde_json::Value, reason: &str) -> TestResult {
    let plan = typed(value)?;
    assert!(
        plan.obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::Execute),
        "{reason}: {:?}",
        plan.obligations
            .iter()
            .map(|ob| &ob.decision)
            .collect::<Vec<_>>()
    );
    assert_eq!(plan.baseline.status(), BaselineStatus::Unavailable);
    assert!(
        has_warning(value, reason),
        "{reason}: {:?}",
        value["plan"]["warnings"]
    );
    Ok(())
}

/// One manifest mutation plus the reason its reuse must report.
type ManifestCase = (&'static str, Box<dyn Fn(&mut serde_json::Value)>);

/// Run every case: each mutation must execute everything with its reason.
fn run_cases(
    root: &std::path::Path,
    seed: &Plan,
    base: &str,
    cases: &[ManifestCase],
) -> TestResult {
    for (reason, mutate) in cases {
        assert_miss(&plan_with_mutation(root, seed, base, mutate)?, reason)?;
    }
    Ok(())
}

/// Mutations breaking manifest-level provenance (commit, ref, event, run,
/// artifact, workflow, compat, generator).
fn manifest_identity_cases() -> Vec<ManifestCase> {
    vec![
        (
            "wrong_commit",
            Box::new(|m| m["source_commit"] = serde_json::Value::String("b".repeat(40))),
        ),
        (
            "wrong_ref",
            Box::new(|m| m["ref"] = serde_json::Value::String("refs/heads/other".to_owned())),
        ),
        (
            "untrusted_proof",
            Box::new(|m| m["event"] = serde_json::Value::String("pull_request".to_owned())),
        ),
        (
            "untrusted_proof",
            Box::new(|m| m["event"] = serde_json::Value::String("merge_group".to_owned())),
        ),
        (
            "untrusted_proof",
            Box::new(|m| m["final_status"] = serde_json::Value::String("failed".to_owned())),
        ),
        (
            "bad_proof_identity",
            Box::new(|m| m["run_id"] = serde_json::Value::from(0)),
        ),
        (
            "bad_proof_identity",
            Box::new(|m| m["run_attempt"] = serde_json::Value::from(0)),
        ),
        (
            "bad_proof_identity",
            Box::new(|m| m["artifact_id"] = serde_json::Value::from(0)),
        ),
        (
            "artifact_mismatch",
            Box::new(|m| m["artifact_name"] = serde_json::Value::String("forged".to_owned())),
        ),
        (
            "generator_mismatch",
            Box::new(|m| m["generator_version"] = serde_json::Value::String("9.9.9".to_owned())),
        ),
        (
            "generator_mismatch",
            Box::new(|m| m["generator_sha256"] = serde_json::Value::String("f".repeat(64))),
        ),
        (
            "generator_unverifiable",
            Box::new(|m| m["generator_sha256"] = serde_json::Value::String("0".repeat(64))),
        ),
        (
            "wrong_workflow",
            Box::new(|m| {
                m["workflow_ref"] =
                    serde_json::Value::String("o/r/other.yml@refs/heads/testmain".to_owned());
            }),
        ),
        (
            "bad_workflow_ref",
            Box::new(|m| m["workflow_ref"] = serde_json::Value::String("bogus".to_owned())),
        ),
        (
            "bad_compatibility_id",
            Box::new(|m| m["compatibility_id"] = serde_json::Value::String("bogus".to_owned())),
        ),
    ]
}

/// Mutations breaking per-task entry binding (identity, run, freshness).
fn task_entry_cases() -> Vec<ManifestCase> {
    vec![
        (
            "bad_task_identity",
            Box::new(|m| {
                m["tasks"][0]["task_digest"] = serde_json::Value::String("bogus".to_owned());
            }),
        ),
        (
            "proof_mismatch",
            Box::new(|m| m["tasks"][0]["observed_run_id"] = serde_json::Value::from(8)),
        ),
        (
            "bad_external_data",
            Box::new(|m| {
                m["tasks"][0]["external_data"] = serde_json::json!({
                    "source": "advisory-db",
                    "identity": "bogus",
                    "age_secs": 60,
                });
            }),
        ),
    ]
}

#[test]
fn every_unsafe_manifest_executes() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let root = repo.path();
    anchor_repo(root)?;
    let base = seed.base.clone().ok_or("base")?;
    run_cases(root, &seed, &base, &manifest_identity_cases())
}

#[test]
fn every_unsafe_task_entry_executes() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let root = repo.path();
    anchor_repo(root)?;
    let base = seed.base.clone().ok_or("base")?;
    run_cases(root, &seed, &base, &task_entry_cases())
}

#[test]
fn carried_proof_binding_mismatch_executes() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let root = repo.path();
    anchor_repo(root)?;
    let base = seed.base.clone().ok_or("base")?;
    let ob = seed.obligations.first().ok_or("obligation")?;
    let value = plan_with_mutation(root, &seed, &base, &|m| {
        m["tasks"][0]["proof"] = serde_json::json!({
            "task_id": ob.task_id,
            "task_digest": velnor_actions_contract::digest_b3(b"other"),
            "input_digest": ob.input_digest,
            "graph_digest": ob.task_digest,
            "toolchain_id": ob.task_digest,
            "mbx_digest": ob.task_digest,
            "platform_id": ob.task_digest,
            "profile": "default",
            "proof_run_id": 7,
        });
    })?;
    assert_miss(&value, "proof_mismatch")?;
    Ok(())
}

#[test]
fn missing_base_is_a_miss_not_a_failure() -> TestResult {
    let (repo, seed) = plan_for_source_change()?;
    let root = repo.path();
    let manifest = manifest_for(&seed, &seed.head, "testmain")?;
    let value = plan_value(root, "pull_request", None, &seed.head, Some(&manifest))?;
    assert_miss(&value, "missing_base")?;
    Ok(())
}

#[test]
fn anchored_repository_binds_and_unanchored_fails_closed() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = crate::support::git_line(&["rev-parse", "HEAD"], root)?;
    let seed = typed(&plan_value(root, "pull_request", Some(&head), &head, None)?)?;
    let mut manifest = manifest_for(&seed, &head, "testmain")?;
    manifest["repository_id"] = serde_json::Value::String(anchor_id());
    // No origin remote: fail closed, never warn-and-cover.
    assert_miss(
        &plan_value(root, "pull_request", Some(&head), &head, Some(&manifest))?,
        "repository_unanchored",
    )?;
    // Anchored origin: exact repository binding enforced.
    anchor_repo(root)?;
    let value = plan_value(root, "pull_request", Some(&head), &head, Some(&manifest))?;
    assert!(!has_warning(&value, "repository_unanchored"));
    assert!(
        typed(&value)?
            .obligations
            .iter()
            .all(|ob| ob.decision == ObligationDecision::CoveredByTrustedBaseline)
    );
    manifest["repository_id"] =
        serde_json::Value::String(velnor_actions_contract::digest_b3(b"github.com/evil/fork"));
    assert_miss(
        &plan_value(root, "pull_request", Some(&head), &head, Some(&manifest))?,
        "wrong_repository",
    )?;
    Ok(())
}

#[test]
fn sharded_plan_binds_archive_content_and_executes() -> TestResult {
    let config = format!(
        "{}\n[test_sharding]\ndefault_shards = 2\n",
        config_with_branch()
    );
    let dir = make_repo(&config)?;
    let root = dir.path();
    write_nextest_task(root)?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let plan = typed(&plan_value(root, "push", Some(&head), &head, None)?)?;
    let mut shards = 0u32;
    for ob in &plan.obligations {
        if !ob.task_id.contains("/shard-") {
            continue;
        }
        shards += 1;
        // The archive identity binds the content closure, so the gate
        // clears on content instead of refusing as source-unbound; the
        // unqualified task cache still executes with its own reason.
        assert_eq!(ob.decision, ObligationDecision::Execute, "{ob:?}");
        assert_ne!(ob.reason, "archive_source_unbound", "{ob:?}");
        assert_eq!(ob.reason, "forced_uncached", "{ob:?}");
    }
    assert!(shards > 0, "{:?}", plan.task_ids);
    Ok(())
}
