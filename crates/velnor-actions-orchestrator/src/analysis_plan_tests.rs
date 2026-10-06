//! Early admission keeps failures typed and preserves the common planner.

use super::*;
#[path = "analysis_plan_fixtures.rs"]
mod fixtures;
use crate::inventory::cargo_probe::CargoProbe;
use fixtures::{TestResult, fixture};

#[test]
fn malformed_requests_are_hard_errors_before_remote_lookup() {
    for text in ["{}", "{", r#"{"schema":1,"schema":2}"#] {
        assert!(test_plan_early(text, |_| unreachable!("invalid request reached lookup")).is_err());
    }
}

#[test]
fn policy_and_helper_failures_never_become_needs_cargo() -> TestResult {
    for failure in [
        "policy",
        "helper_missing",
        "helper_digest",
        "helper_version",
    ] {
        let fixture = fixture()?;
        let root = fixture.repo.path();
        match failure {
            "policy" => std::fs::write(
                root.join(".velnor/config.toml"),
                "schema = 1\n[workflow]\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"main\"\n",
            )?,
            "helper_missing" => std::fs::remove_file(root.join(".velnor/release-manifest.json"))?,
            _ => {
                let path = root.join(".velnor/release-manifest.json");
                let mut manifest: serde_json::Value =
                    serde_json::from_str(&std::fs::read_to_string(&path)?)?;
                if failure == "helper_digest" {
                    for target in manifest["targets"].as_array_mut().ok_or("targets")? {
                        target["sha256"] = "d".repeat(64).into();
                    }
                } else {
                    manifest["version"] = "0.2.0".into();
                    for target in manifest["targets"].as_array_mut().ok_or("targets")? {
                        target["artifact"] = target["artifact"]
                            .as_str()
                            .ok_or("artifact")?
                            .replace("0.1.0", "0.2.0")
                            .into();
                    }
                }
                std::fs::write(path, manifest.to_string())?;
            }
        }
        let probe = CargoProbe::begin();
        let result = test_plan_early(&fixture.request.to_string(), |_| unreachable!("{failure}"));
        assert!(result.is_err(), "{failure}: {result:?}");
        assert_eq!(probe.attempts(), 0);
    }
    Ok(())
}

#[test]
fn remote_miss_and_resolution_mismatch_explicitly_require_cargo() -> TestResult {
    let fixture = fixture()?;
    let probe = CargoProbe::begin();
    let result = test_plan_early(&fixture.request.to_string(), |_| {
        Err("analysis_run_mismatch".into())
    })?;
    assert!(
        matches!(result, EarlyPlanResult::NeedsCargo {reason} if reason == "analysis_run_mismatch")
    );
    std::fs::write(fixture.repo.path().join("Cargo.lock"), "version = 4\n")?;
    let result = test_plan_early(&fixture.request.to_string(), |_| Ok(fixture.download()))?;
    assert!(matches!(result, EarlyPlanResult::NeedsCargo { .. }));
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn uncovered_plan_owned_rust_obligation_requires_cargo() -> TestResult {
    let fixture = fixture()?;
    let probe = CargoProbe::begin();
    let result = test_plan_early(&fixture.request.to_string(), |_| Ok(fixture.download()))?;
    assert!(
        matches!(result, EarlyPlanResult::NeedsCargo {reason}
        if reason == "plan_rust_obligation_uncovered"),
        "unexpected result"
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn covered_ready_matches_normal_planner_and_same_prep_generates_freshly_without_cargo() -> TestResult
{
    let mut fixture = fixture()?;
    let probe = CargoProbe::begin();
    let prep = fixture.prep()?;
    crate::generate::generate(&prep, &crate::generate::GenerateOptions::default())?;
    fixture.commit_current()?;
    fixture.cover_request()?;
    let prep = fixture.prep()?;
    test_check_freshness(&prep)?;
    let normal = fixture.normal()?;
    let result = test_plan_early(&fixture.request.to_string(), |_| Ok(fixture.download()))?;
    let EarlyPlanResult::Ready { response } = result else {
        return Err(format!("{result:?}").into());
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&response)?,
        serde_json::from_str::<serde_json::Value>(&normal)?
    );
    let decoded: super::super::PlanResponse = serde_json::from_str(&response)?;
    assert!(decoded.baseline_manifest.is_some());
    assert!(
        decoded
            .plan
            .obligations
            .iter()
            .any(|obligation| obligation.decision
                == velnor_actions_contract::ObligationDecision::CoveredByTrustedBaseline)
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn authenticated_preparation_errors_remain_hard_without_fallback() -> TestResult {
    let mut fixture = fixture()?;
    std::fs::write(
        fixture.repo.path().join("package.json"),
        r#"{"name":"undeclared-native-workload"}"#,
    )?;
    fixture.refresh_analysis()?;
    let probe = CargoProbe::begin();
    let error = test_plan_early(&fixture.request.to_string(), |_| Ok(fixture.download()))
        .expect_err("native workload declaration is required");
    assert!(
        error.to_string().contains("native_obligation_undeclared"),
        "{error}"
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn promotion_rejects_all_request_and_generator_dimensions_before_remote_lookup() -> TestResult {
    let mut fixture = fixture()?;
    fixture.cover_request()?;
    let response: serde_json::Value = serde_json::from_str(&fixture.normal()?)?;
    for (pointer, replacement) in [
        ("/plan/run_key", serde_json::json!("r3-a1")),
        ("/plan/head", serde_json::json!("d".repeat(40))),
        ("/plan/base", serde_json::json!("d".repeat(40))),
        ("/plan/scope", serde_json::json!("full")),
        ("/plan/generator/sha256", serde_json::json!("d".repeat(64))),
        ("/plan/generator/version", serde_json::json!("0.2.0")),
        ("/plan/generator/target", serde_json::json!("wrong-target")),
        ("/matrix/include", serde_json::json!([{"invalid":"entry"}])),
    ] {
        let mut altered = response.clone();
        *altered.pointer_mut(pointer).ok_or("response pointer")? = replacement;
        let probe = CargoProbe::begin();
        assert!(
            validate_early_response(&fixture.request.to_string(), &altered.to_string()).is_err(),
            "{pointer}"
        );
        assert_eq!(probe.attempts(), 0);
    }
    Ok(())
}

#[test]
fn authenticated_payload_identity_mismatch_requires_cargo_without_planning() -> TestResult {
    for pointer in [
        "/identity/helper_sha256",
        "/identity/source/head_sha",
        "/identity/cargo_pin",
    ] {
        let mut fixture = fixture()?;
        let mut payload: serde_json::Value = serde_json::from_str(&fixture.text)?;
        *payload.pointer_mut(pointer).ok_or("identity pointer")? = "wrong".into();
        fixture.text = payload.to_string();
        let probe = CargoProbe::begin();
        let result = test_plan_early(&fixture.request.to_string(), |_| Ok(fixture.download()))?;
        assert!(
            matches!(result, EarlyPlanResult::NeedsCargo { .. }),
            "{pointer}"
        );
        assert_eq!(probe.attempts(), 0);
    }
    Ok(())
}

#[test]
fn promotion_reauthenticates_identical_ready_and_rejects_valid_changed_plan_content() -> TestResult
{
    let mut fixture = fixture()?;
    fixture.cover_request()?;
    let staged = fixture.normal()?;
    let probe = CargoProbe::begin();
    let promoted = test_validate_early(&fixture.request.to_string(), &staged, |inputs| {
        assert_eq!(inputs.base, fixture.identity.source.head_sha);
        assert_eq!(inputs.helper_sha256, fixture.identity.helper_sha256);
        Ok(fixture.download())
    })?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&promoted)?,
        serde_json::from_str::<serde_json::Value>(&staged)?
    );
    let mut altered: serde_json::Value = serde_json::from_str(&staged)?;
    altered["plan"]["warnings"] = serde_json::json!(["forged-but-valid-warning"]);
    let error = test_validate_early(&fixture.request.to_string(), &altered.to_string(), |_| {
        Ok(fixture.download())
    })
    .expect_err("whole response equality required");
    assert!(
        error.to_string().contains("early_response_stale"),
        "{error}"
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn promotion_rejects_changed_source_even_when_head_and_resolution_stay_identical() -> TestResult {
    let mut fixture = fixture()?;
    fixture.cover_request()?;
    let staged = fixture.normal()?;
    let original_head = fixtures::git(fixture.repo.path(), &["rev-parse", "HEAD"])?;
    std::fs::write(
        fixture.repo.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 43 }\n",
    )?;
    assert_eq!(
        fixtures::git(fixture.repo.path(), &["rev-parse", "HEAD"])?,
        original_head
    );
    let probe = CargoProbe::begin();
    let error = test_validate_early(&fixture.request.to_string(), &staged, |_| {
        Ok(fixture.download())
    })
    .expect_err("changed closure cannot promote");
    assert!(
        matches!(error, OrchestratorError::NeedsCargo { .. })
            || error.to_string().contains("early_response_stale"),
        "{error}"
    );
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn promotion_remote_admission_miss_is_error_and_never_fresh_cargo_fallback() -> TestResult {
    let mut fixture = fixture()?;
    fixture.cover_request()?;
    let staged = fixture.normal()?;
    let probe = CargoProbe::begin();
    let error = test_validate_early(&fixture.request.to_string(), &staged, |_| {
        Err("analysis_remote_authority_lost".to_owned())
    })
    .expect_err("reauthentication required");
    assert!(matches!(error, OrchestratorError::NeedsCargo {problem}
        if problem == "analysis_remote_authority_lost"));
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[test]
fn hosted_ready_gates_rust_mbx_preparation_in_rendered_workflow() -> TestResult {
    use velnor_actions_contract::PLAN_JOB_ID;
    use velnor_actions_workflow_renderer::early_plan::NEEDS_CARGO_CONDITION;
    let fixture = fixture()?;
    let mut prep = fixture.prep()?;
    let acquire = prep
        .workflow
        .ir
        .jobs
        .get(PLAN_JOB_ID)
        .ok_or("plan job")?
        .steps
        .get(1)
        .ok_or("release acquisition")?
        .clone();
    let plan = crate::workflow_jobs::plan_job(
        &prep.runner_label,
        Some(acquire),
        &ToolCatalog::pinned(),
        true,
        true,
        true,
        false,
        &[String::new()],
    )?;
    assert_ready_gates(&plan)?;
    prep.workflow.ir.jobs.insert(PLAN_JOB_ID.to_owned(), plan);
    let tree = crate::generate::render_staged_tree(&prep)?;
    let yaml = tree
        .get(velnor_actions_workflow_renderer::render::WORKFLOW_PATH)
        .ok_or("workflow file")?;
    assert!(yaml.contains("name: Plan"), "rendered plan job");
    Ok(())
}

fn assert_ready_gates(plan: &velnor_actions_contract::Job) -> TestResult {
    use velnor_actions_contract::StepKind;
    use velnor_actions_workflow_renderer::early_plan::NEEDS_CARGO_CONDITION;
    let expensive = plan
        .steps
        .iter()
        .filter(|step| {
            step.name == velnor_actions_mise::PREPARE_PINNED_TOOLS_STEP
                || step.name == velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP
                || step
                    .name
                    .starts_with(crate::source_prep::FETCH_SOURCES_STEP)
        })
        .collect::<Vec<_>>();
    for step in expensive {
        assert!(
            step.condition
                .as_ref()
                .is_some_and(|condition| condition.contains(NEEDS_CARGO_CONDITION)),
            "{} must skip Ready",
            step.name
        );
    }
    let planning = plan
        .steps
        .iter()
        .find(|step| step.name == "Prepare planning tools")
        .ok_or("planning tools")?;
    let StepKind::Shell { run, env } = &planning.kind else {
        return Err("planning shell".into());
    };
    assert!(
        run.iter()
            .all(|word| !word.contains("rust@") && !word.contains("mr-boxington@"))
    );
    assert!(!env.contains_key("RUSTUP_HOME"));
    Ok(())
}

#[test]
fn remotely_authenticated_unqualified_cargo_commit_requires_cargo() -> TestResult {
    let mut fixture = fixture()?;
    fixture.identity.cargo_identity = fixture.identity.cargo_identity.replace(
        crate::analysis_inventory::QUALIFIED_CARGO_COMMIT_PREFIX,
        "012345678",
    );
    let mut payload: serde_json::Value = serde_json::from_str(&fixture.text)?;
    payload["identity"]["cargo_identity"] = fixture.identity.cargo_identity.clone().into();
    fixture.text = payload.to_string();
    let probe = CargoProbe::begin();
    let result = test_plan_early(&fixture.request.to_string(), |_| Ok(fixture.download()))?;
    assert!(matches!(result, EarlyPlanResult::NeedsCargo {reason}
        if reason == "analysis_inventory_invalid_identity"));
    assert_eq!(probe.attempts(), 0);
    Ok(())
}

#[path = "analysis_native_tests.rs"]
mod native_tests;
