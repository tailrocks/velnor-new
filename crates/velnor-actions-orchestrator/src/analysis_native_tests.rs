//! No-Rust repositories admit current native/Tofu work without Cargo artifacts.

use super::fixtures::{git, release_manifest, task_proof};
use super::*;

struct NativeFixture {
    repo: tempfile::TempDir,
    request: serde_json::Value,
}

fn native_fixture(kind: &str) -> Result<NativeFixture, Box<dyn std::error::Error>> {
    let repo = tempfile::tempdir()?;
    let root = repo.path().canonicalize()?;
    std::fs::create_dir(root.join(".velnor"))?;
    let settings = match kind {
        "tofu" => {
            std::fs::write(
                root.join("main.tf"),
                "terraform {}\nlocals { answer = 42 }\n",
            )?;
            "[stacks.tofu]\nroots = [\".\"]\n"
        }
        "swift" => {
            std::fs::write(
                root.join("Package.swift"),
                "// swift-tools-version: 6.0\nimport PackageDescription\nlet package = Package(name: \"Demo\")\n",
            )?;
            "[[stacks.workloads]]\nname = \"native\"\nkind = \"swift_test\"\ninputs = [\"Package.swift\"]\n"
        }
        _ => return Err("unknown fixture kind".into()),
    };
    std::fs::write(
        root.join(".velnor/config.toml"),
        format!("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"main\"\n{settings}"),
    )?;
    release_manifest(&root)?;
    git(&root, &["init", "--initial-branch=main"])?;
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/demo.git",
        ],
    )?;
    git(&root, &["add", "."])?;
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "native"],
    )?;
    let head = git(&root, &["rev-parse", "HEAD"])?;
    let request = serde_json::json!({"schema":1,"op":"plan-v1","run_key":"r2-a1",
        "root":root,"repository":"example/demo","event":"local","scope":"affected",
        "base":null,"head":head});
    Ok(NativeFixture { repo, request })
}

fn no_rust_ready(request: &str) -> Result<String, Box<dyn std::error::Error>> {
    match test_plan_early(request, |_| {
        unreachable!("native planning needs no Rust analysis artifact")
    })? {
        EarlyPlanResult::Ready { response } => Ok(response),
        result => Err(format!("native admission failed: {result:?}").into()),
    }
}

#[test]
fn native_and_tofu_ready_match_fresh_common_plan_and_promote_without_lookup_or_cargo() -> TestResult
{
    for kind in ["swift", "tofu"] {
        let fixture = native_fixture(kind)?;
        let probe = CargoProbe::begin();
        let fresh = crate::internal::plan_internal(&fixture.request.to_string())?;
        let early = no_rust_ready(&fixture.request.to_string())?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&fresh)?,
            serde_json::from_str::<serde_json::Value>(&early)?,
            "{kind}"
        );
        let response: crate::internal::PlanResponse = serde_json::from_str(&early)?;
        assert!(
            !response.plan.obligations.is_empty(),
            "{kind} work must remain explicit"
        );
        assert!(
            response
                .plan
                .task_ids
                .iter()
                .all(|id| !id.starts_with("stack/rust/"))
        );
        let promoted = test_validate_early(&fixture.request.to_string(), &early, |_| {
            unreachable!("native promotion needs no Rust artifact")
        })?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&early)?,
            serde_json::from_str::<serde_json::Value>(&promoted)?
        );
        assert_eq!(probe.attempts(), 0);
    }
    Ok(())
}

#[test]
fn native_full_scope_ready_needs_neither_base_nor_rust_inventory() -> TestResult {
    let mut fixture = native_fixture("swift")?;
    fixture.request["scope"] = "full".into();
    let probe = CargoProbe::begin();
    let response = no_rust_ready(&fixture.request.to_string())?;
    let plan: crate::internal::PlanResponse = serde_json::from_str(&response)?;
    assert_eq!(plan.plan.scope, VerificationScope::Full);
    assert!(plan.plan.base.is_none());
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn no_rust_malformed_configuration_stays_hard_error() -> TestResult {
    let fixture = native_fixture("tofu")?;
    std::fs::write(
        fixture.repo.path().join(".velnor/config.toml"),
        "schema = 1\nunknown = true\n",
    )?;
    let probe = CargoProbe::begin();
    assert!(
        test_plan_early(&fixture.request.to_string(), |_| unreachable!(
            "bad config lookup"
        ))
        .is_err()
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn new_rust_candidate_prevents_native_only_admission() -> TestResult {
    let fixture = native_fixture("tofu")?;
    std::fs::write(
        fixture.repo.path().join("Cargo.toml"),
        "[package]\nname = \"rust-work\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    let probe = CargoProbe::begin();
    let result = test_plan_early(&fixture.request.to_string(), |_| {
        unreachable!("no base before lookup")
    })?;
    assert!(
        matches!(result, EarlyPlanResult::NeedsCargo {reason} if reason == "analysis_base_absent")
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

fn baseline_for(
    plan: &velnor_actions_contract::Plan,
    base: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let compat = crate::cover_compat::baseline_compat_for_plan(plan)?;
    let name = crate::cover_baseline::lookup_artifact_name(plan, base)?;
    let tasks = plan
        .obligations
        .iter()
        .map(|obligation| {
            let proof = task_proof(obligation, 1)?;
            Ok(serde_json::json!({
                "task_id":obligation.task_id,"task_digest":obligation.task_digest,
                "input_digest":obligation.input_digest,"closure_digest":obligation.closure_digest,
                "proof_run_id":1,"observed_run_id":1,"carried_from":null,"proof":proof,
            }))
        })
        .collect::<Result<Vec<_>, velnor_actions_contract::ContractError>>()?;
    Ok(serde_json::json!({
        "schema":crate::internal_plan::snapshot::CANONICAL_SCHEMA_VERSION,"repository_id":velnor_actions_contract::digest_b3(b"github.com/example/demo"),
        "source_commit":base,"ref":"refs/heads/main","event":"push",
        "workflow_ref":"example/demo/.github/workflows/ci.yml@refs/heads/main",
        "run_id":1,"run_attempt":1,"final_status":"passed",
        "generator_version":plan.generator.version,"generator_sha256":plan.generator.sha256,
        "compatibility_id":compat,"artifact_id":crate::cover_compat::baseline_artifact_numeric_id(&name),
        "artifact_name":name,"parent":null,"expires_at_unix":null,"tasks":tasks,
    }))
}

#[test]
fn tofu_ready_preserves_verified_baseline_coverage_without_rust_artifact() -> TestResult {
    let mut fixture = native_fixture("tofu")?;
    let probe = CargoProbe::begin();
    let first = no_rust_ready(&fixture.request.to_string())?;
    let response: crate::internal::PlanResponse = serde_json::from_str(&first)?;
    let head = fixture.request["head"].as_str().ok_or("head")?.to_owned();
    fixture.request["base"] = head.clone().into();
    fixture.request["baseline_manifest"] = baseline_for(&response.plan, &head)?;
    let covered = no_rust_ready(&fixture.request.to_string())?;
    let plan: crate::internal::PlanResponse = serde_json::from_str(&covered)?;
    assert!(plan.baseline_manifest.is_some());
    assert!(
        plan.plan
            .obligations
            .iter()
            .any(|obligation| obligation.decision
                == velnor_actions_contract::ObligationDecision::CoveredByTrustedBaseline)
    );
    let fresh = crate::internal::plan_internal(&fixture.request.to_string())?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&fresh)?,
        serde_json::from_str::<serde_json::Value>(&covered)?
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn stale_no_rust_detector_cannot_authorize_cargo_after_checkout_changes() -> TestResult {
    let fixture = native_fixture("tofu")?;
    let (request, root) = resolved_request(&fixture.request.to_string(), crate::internal::PLAN_OP)?;
    let config = load_config(&root)?;
    let (stale_index, _) = build_file_index(&root, &config.discovery.exclude)?;
    assert!(velnor_actions_rust::discover_stack_candidates(&stale_index).is_empty());
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"new-rust\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    let probe = CargoProbe::begin();
    let error = super::super::admission::plan_without_rust(&request, &root, &config, &stale_index)
        .expect_err("fresh discovery must refuse stale no-Rust admission");
    assert!(matches!(error, OrchestratorError::NeedsCargo {problem}
        if problem == "rust_discovered_after_no_cargo_admission"));
    assert_eq!(probe.attempts(), 0);
    Ok(())
}
