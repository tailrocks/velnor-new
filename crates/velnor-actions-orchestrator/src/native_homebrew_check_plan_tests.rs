use super::*;
use crate::internal_plan::native_homebrew_checks::plan_envelope::derive_plan_envelopes;
use crate::internal_plan::native_homebrew_checks::{
    IDENTITY_SCHEMA, NativeCheckAuthorityIdentity, NativeCheckIdentityExtension,
};
use velnor_actions_contract::config::{CheckExecutor, CheckRunner};
use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_contract::{
    CheckPlatform, ExecuteTaskRef, JobDisposition, ObligationDecision, Plan, PlanJob, PlanMatrix,
    PlanRunner, RunnerSelection, Trust, plan_id_for_run,
};

fn native_check(operation: NativeHomebrewOperation) -> NativeHomebrewCheck {
    NativeHomebrewCheck {
        id: operation.as_str().replace('_', "-"),
        operation,
        formula_channel: matches!(
            operation,
            NativeHomebrewOperation::FormulaContract | NativeHomebrewOperation::Install
        )
        .then_some(PackageReleaseChannel::Preview),
        runner: CheckRunner {
            label: "macos-15".to_owned(),
            platform: CheckPlatform::MacosArm64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        timeout_minutes: 1,
    }
}

fn native_identity(check: &NativeHomebrewCheck) -> NativeCheckIdentityExtension {
    let formula_path = check.formula_channel.map(|_| "Formula/demo.rb".to_owned());
    NativeCheckIdentityExtension {
        schema: IDENTITY_SCHEMA.to_owned(),
        check_id: check.id.clone(),
        operation: check.operation,
        formula_channel: check.formula_channel,
        operation_version: OPERATION_VERSION.to_owned(),
        sdk_version: "0.1.53".to_owned(),
        sdk_sha256: "a".repeat(64),
        authority: NativeCheckAuthorityIdentity::Source {
            source_repository_id: 123_456,
            authority_sha: "b".repeat(40),
            source_plan_digest: format!("b3-{}", "c".repeat(64)),
            locator_blob_sha: "d".repeat(40),
            formula_spec_digest: format!("b3-{}", "e".repeat(64)),
            cutover: None,
        },
        tap_repository: "example/homebrew-tap".to_owned(),
        tap_repository_id: 123_456,
        tap_base_sha: "f".repeat(40),
        head: "1".repeat(40),
        tree_object: "2".repeat(40),
        formula_blob_sha: formula_path.as_ref().map(|_| "3".repeat(40)),
        formula_path,
        runner: check.runner.label.clone(),
        platform: check.runner.platform,
        tool_profile: "native-homebrew-check-v1".to_owned(),
    }
}

fn valid_plan(generator: &PlanGenerator, obligation: PlanObligation, entry: MatrixEntry) -> Plan {
    Plan {
        schema: Plan::SCHEMA,
        run_key: "local".to_owned(),
        plan_id: plan_id_for_run("local").expect("plan ID"),
        base: None,
        head: "1".repeat(40),
        event: WorkflowEvent::PullRequest,
        qualification: None,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        generator: generator.clone(),
        packages: Vec::new(),
        obligations: vec![obligation.clone()],
        matrix: PlanMatrix {
            include: vec![entry.clone()],
        },
        task_ids: vec![obligation.task_id],
        jobs: vec![PlanJob {
            job_id: entry.job_id,
            disposition: JobDisposition::Run,
        }],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

#[test]
fn native_proposals_derive_valid_execute_maps_for_every_operation() {
    let generator = PlanGenerator {
        version: "0.1.53".to_owned(),
        target: "aarch64-apple-darwin".to_owned(),
        sha256: "a".repeat(64),
    };
    for operation in [
        NativeHomebrewOperation::TapTree,
        NativeHomebrewOperation::FormulaContract,
        NativeHomebrewOperation::Install,
        NativeHomebrewOperation::AdoptionContract,
    ] {
        let check = native_check(operation);
        let task = proposal(&check).expect("native proposal");
        assert_eq!(task.task_kind, "native_check");
        assert_eq!(task.task_id, format!("internal/native-check/{}", check.id));
        validate_task_binding(&check, &task).expect("canonical native task binding");

        let (obligation, entries) =
            derive_plan_envelopes(&check, &task, "local", &generator, &native_identity(&check))
                .expect("derive plan envelopes");
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.execute_task_ids.tasks.len(), 1);
        assert!(matches!(
            entry.execute_task_ids.tasks.get("native_check"),
            Some(ExecuteTaskRef::Single(task_id)) if task_id == &task.task_id
        ));
        entry
            .execute_task_ids
            .validate()
            .expect("native execute-task map");
        entry.validate("local").expect("native matrix entry");
        assert_eq!(obligation.task_id, task.task_id);
        assert_eq!(obligation.decision, ObligationDecision::Execute);

        let plan = valid_plan(&generator, obligation, entry.clone());
        plan.validate().expect("native obligation and plan");
        let wire = serde_json::to_vec(&plan).expect("serialize plan wire");
        serde_json::from_slice::<Plan>(&wire)
            .expect("deserialize plan wire")
            .validate()
            .expect("validate decoded plan wire");
    }
}

#[test]
fn native_task_kind_mutation_is_rejected() {
    let check = native_check(NativeHomebrewOperation::TapTree);
    let mut task = proposal(&check).expect("native proposal");
    task.task_kind = "native-check".to_owned();
    assert!(validate_task_binding(&check, &task).is_err());

    let generator = PlanGenerator {
        version: "0.1.53".to_owned(),
        target: "aarch64-apple-darwin".to_owned(),
        sha256: "a".repeat(64),
    };
    assert!(
        derive_plan_envelopes(&check, &task, "local", &generator, &native_identity(&check),)
            .is_err()
    );
}
