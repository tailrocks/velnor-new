use super::*;

use crate::ids::{plan_id_for_run, run_key_for_ci};
use crate::workflow::{
    EntryCacheIds, ExecuteTaskIds, MatrixEntry, ObligationDecision, Plan, PlanBaseline,
    PlanGenerator, PlanMatrix, PlanObligation, PlanPackage, PlanRunner, PlannedPlatform,
    QualificationDispatch, QualificationPhase, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityRequirements, QualificationRuntimePlatform, WorkflowEvent,
};
use crate::{RunnerSelection, Trust, digest_b3};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn serialized_control_and_task_result_saves_cannot_authorize_cache_access() -> TestResult {
    for (phase, layer) in [
        (
            QualificationPhase::Control,
            QualificationCacheLayer::CargoSources,
        ),
        (
            QualificationPhase::Cold,
            QualificationCacheLayer::TaskResult,
        ),
    ] {
        let plan = qualification_plan(phase, 700, &"a".repeat(40), None)?;
        let entry = &plan.matrix.include[0];
        let directive = QualificationCacheDirective::for_plan(&plan, None)?
            .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
        let layer_directive = directive
            .layer(&entry.matrix_key, layer)
            .ok_or_else(|| std::io::Error::other("layer directive missing"))?;
        let evidence = runtime_identity(&layer_directive.runtime);
        let mut serialized = serde_json::to_value(&directive)?;
        let layers = serialized["lanes"][0]["layers"]
            .as_array_mut()
            .ok_or_else(|| std::io::Error::other("layers are not an array"))?;
        let forged = layers
            .iter_mut()
            .find(|value| value["layer"] == serde_json::json!(layer))
            .ok_or_else(|| std::io::Error::other("target layer missing"))?;
        forged["active"] = serde_json::json!(true);
        forged["restore"] = serde_json::json!({"slot":"k1","expected_cache":null});
        forged["restore_policy"] = serde_json::json!("admission_gated");
        forged["save_policy"] = serde_json::json!("k1");

        let forged: QualificationCacheDirective = serde_json::from_value(serialized)?;
        assert!(
            forged
                .bind_runtime(&plan, None, &entry.matrix_key, layer, &evidence)
                .is_err(),
            "{phase:?} forged {layer:?} directive must fail plan reconstruction"
        );
    }
    Ok(())
}

#[test]
fn plan_reconstructed_cold_directive_binds_runtime_identity() -> TestResult {
    let plan = qualification_plan(QualificationPhase::Cold, 700, &"a".repeat(40), None)?;
    plan.validate()?;
    let entry = &plan.matrix.include[0];
    let directive = QualificationCacheDirective::for_plan(&plan, None)?
        .ok_or_else(|| std::io::Error::other("qualification directive missing"))?;
    let layer_directive = directive
        .layer(&entry.matrix_key, QualificationCacheLayer::CargoSources)
        .ok_or_else(|| std::io::Error::other("layer directive missing"))?;
    let keys = directive.bind_runtime(
        &plan,
        None,
        &entry.matrix_key,
        QualificationCacheLayer::CargoSources,
        &runtime_identity(&layer_directive.runtime),
    )?;
    assert!(keys.restore.is_some());
    assert!(keys.save_key.is_some());
    Ok(())
}

pub(super) fn qualification_plan(
    phase: QualificationPhase,
    run_id: u64,
    source_sha: &str,
    predecessor: Option<crate::workflow::QualificationRunRef>,
) -> Result<Plan, crate::ContractError> {
    let package_id = "path+file:///workspace/crates/demo#demo@0.1.0".to_owned();
    let task_digest = digest_b3(b"test task");
    let input_digest = digest_b3(b"test input");
    let run_key = run_key_for_ci(run_id, 1);
    let entry = qualification_entry(&package_id, &task_digest, &input_digest, &run_key)?;
    let context = qualification_context(phase, source_sha, run_id, predecessor);
    Ok(Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key)?,
        base: None,
        head: source_sha.to_owned(),
        event: WorkflowEvent::Qualification,
        qualification: Some(context),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        baseline: PlanBaseline::unavailable(Some("qualification_bypass"))?,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "c".repeat(64),
        },
        packages: vec![PlanPackage {
            package_id,
            name: "demo".to_owned(),
            manifest: "crates/demo/Cargo.toml".to_owned(),
            selected: true,
            reasons: vec!["changed".to_owned()],
            tasks: vec!["stack/rust/demo/test/default".to_owned()],
        }],
        obligations: vec![PlanObligation {
            task_id: "stack/rust/demo/test/default".to_owned(),
            decision: ObligationDecision::Execute,
            reason: "qualification_full".to_owned(),
            task_digest: task_digest.clone(),
            input_digest,
            closure_digest: digest_b3(b"closure"),
            baseline_proof: None,
        }],
        matrix: PlanMatrix {
            include: vec![entry],
        },
        task_ids: vec!["stack/rust/demo/test/default".to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    })
}

fn qualification_entry(
    package_id: &str,
    task_digest: &str,
    input_digest: &str,
    run_key: &str,
) -> Result<MatrixEntry, crate::ContractError> {
    let planned_platform = PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")?;
    let mut entry = MatrixEntry::derive(
        "rust",
        "stack/rust/demo/test/default",
        "mise run test",
        task_digest,
        serde_json::json!({
            "package_id": package_id,
            "unit_id": package_id,
            "compile_driver": "cargo"
        }),
        ExecuteTaskIds::default(),
        input_digest,
        run_key,
        "rust-demo",
        planned_platform.clone(),
    )?;
    entry.cache_ids = Some(EntryCacheIds::new(
        &digest_b3(b"workspace"),
        &digest_b3(b"lane"),
        planned_platform.platform_id.as_str(),
        &digest_b3(b"toolchain"),
        &digest_b3(b"cache format"),
    )?);
    Ok(entry)
}

fn qualification_context(
    phase: QualificationPhase,
    source_sha: &str,
    run_id: u64,
    predecessor: Option<crate::workflow::QualificationRunRef>,
) -> QualificationDispatch {
    QualificationDispatch {
        campaign: "directive-boundary".to_owned(),
        phase,
        repository: "tailrocks/velnor-new".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: source_sha.to_owned(),
        source_sha: source_sha.to_owned(),
        run_id,
        run_attempt: 1,
        predecessor,
    }
}

pub(super) fn runtime_identity(
    requirements: &QualificationRuntimeIdentityRequirements,
) -> QualificationRuntimeIdentity {
    QualificationRuntimeIdentity {
        platform: QualificationRuntimePlatform {
            os: "linux".to_owned(),
            arch: "x86_64".to_owned(),
            runs_on: requirements.runner_label.clone(),
            image_os: "ubuntu".to_owned(),
            image_version: "26.04".to_owned(),
            target: requirements.target.clone(),
        },
        toolchain_id: requirements.toolchain_id.clone(),
        cache_format_id: requirements.cache_format_id.clone(),
        abi_id: digest_b3(b"abi"),
        driver_id: requirements.driver_id.clone(),
    }
}
