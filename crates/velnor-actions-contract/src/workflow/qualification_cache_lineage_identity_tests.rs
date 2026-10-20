use super::*;

use crate::workflow::{
    EntryCacheIds, ExecuteTaskIds, MatrixEntry, ObligationDecision, PlanGenerator, PlanMatrix,
    PlanObligation, PlanPackage, PlanRunner, PlannedPlatform, QualificationDispatch,
    QualificationDispatchMode, QualificationPhase, QualificationRunRef, TaskBindingDigest,
    WorkflowEvent,
};
use crate::{
    RunnerSelection, Trust,
    cachekey::{ToolchainInputs, toolchain_id},
    digest_b3,
    ids::{plan_id_for_run, run_key_for_ci},
};

#[test]
fn qualification_cache_keys_use_the_current_format_namespace() {
    let key = key_for_slot(
        "identity-test",
        QualificationCacheLayer::CargoSources,
        &digest_b3(b"identity"),
        &digest_b3(b"runtime"),
        QualificationCacheSlot::K1,
    )
    .expect("cache key");

    let prefix = format!(
        "velnor-qcache-v{}-",
        crate::workflow::QUALIFICATION_CACHE_FORMAT_VERSION
    );
    assert!(key.starts_with(&prefix));
    assert!(!key.starts_with("velnor-qcache-v2-"));
}

#[test]
fn cache_identity_survives_checkout_relocation_and_useful_source_edit() {
    let before = plan_for_checkout(
        "/home/runner/work/velnor/velnor",
        44,
        "a".repeat(40),
        "input-before",
        "closure-before",
    );
    let after = plan_for_checkout(
        "/tmp/agent/_work/velnor/velnor",
        45,
        "b".repeat(40),
        "input-after-source-edit",
        "closure-after-source-edit",
    );
    let before_matrix = &before.matrix.include[0];
    let after_matrix = &after.matrix.include[0];

    assert_eq!(
        configuration_commitment(&before).expect("before configuration"),
        configuration_commitment(&after).expect("after configuration")
    );
    assert_eq!(
        identity_commitment(
            &before,
            before_matrix,
            QualificationCacheLayer::CargoSources
        )
        .expect("before key identity"),
        identity_commitment(&after, after_matrix, QualificationCacheLayer::CargoSources)
            .expect("after key identity")
    );
    let before_owner =
        owner_for_plan(&before, "rust-demo").expect("The owner before the change must exist.");
    let after_owner =
        owner_for_plan(&after, "rust-demo").expect("The owner after the change must exist.");
    assert_ne!(before_owner.member_digest, after_owner.member_digest);
    assert_eq!(
        owner_layer_identity(
            &before,
            &before_owner,
            QualificationCacheLayer::CargoSources
        )
        .expect("The owner layer identity before the change must exist."),
        owner_layer_identity(&after, &after_owner, QualificationCacheLayer::CargoSources)
            .expect("The owner layer identity after the change must exist.")
    );
}

#[test]
fn physical_owner_digest_commits_every_ordered_member() {
    let mut plan = plan_for_checkout(
        "/workspace/velnor",
        44,
        "a".repeat(40),
        "input-before",
        "closure-before",
    );
    let mut second = plan.matrix.include[0].clone();
    second.id = "stack:rust|task:demo-lint".to_owned();
    second.matrix_key = "matrix-demo-lint".to_owned();
    second.task_id = "demo-lint".to_owned();
    second.input_digest = digest_b3(b"second member input");
    plan.matrix.include.push(second);

    let owner_id = plan.matrix.include[0].job_id.clone();
    let owner = owner_for_plan(&plan, &owner_id).expect("The complete owner must exist.");
    assert_eq!(owner.members.len(), 2);
    assert_eq!(owner.members[0].matrix_key, "matrix-demo-lint");
    assert_eq!(owner.members[1].matrix_key, "matrix-demo-test");

    let mut reordered = plan.clone();
    reordered.matrix.include.reverse();
    assert_eq!(
        owner.member_digest,
        owner_for_plan(&reordered, &owner_id)
            .expect("The reordered owner must exist.")
            .member_digest
    );

    let mut changed = plan;
    changed.matrix.include[0].input_digest = digest_b3(b"changed second member input");
    assert_ne!(
        owner.member_digest,
        owner_for_plan(&changed, &owner_id)
            .expect("The changed owner must exist.")
            .member_digest
    );

    let mut changed_task = plan_for_checkout(
        "/workspace/velnor",
        44,
        "a".repeat(40),
        "input-before",
        "closure-before",
    );
    let mut second = changed_task.matrix.include[0].clone();
    second.id = "stack:rust|task:demo-lint".to_owned();
    second.matrix_key = "matrix-demo-lint".to_owned();
    second.task_id = "demo-lint".to_owned();
    second.input_digest = digest_b3(b"second member input");
    changed_task.matrix.include.push(second);
    let baseline = owner_for_plan(&changed_task, &owner_id).expect("The task owner must exist.");
    changed_task.obligations[0].closure_digest = digest_b3(b"changed task closure");
    assert_ne!(
        baseline.member_digest,
        owner_for_plan(&changed_task, &owner_id)
            .expect("The changed task owner must exist.")
            .member_digest
    );
}

#[test]
fn physical_owner_rejects_members_with_different_platforms() {
    let mut plan = plan_for_checkout(
        "/workspace/velnor",
        44,
        "a".repeat(40),
        "input-before",
        "closure-before",
    );
    let mut second = plan.matrix.include[0].clone();
    second.id = "stack:rust|task:demo-lint".to_owned();
    second.matrix_key = "matrix-demo-lint".to_owned();
    second.task_id = "demo-lint".to_owned();
    second.planned_platform =
        PlannedPlatform::new("macos-26", "aarch64-apple-darwin").expect("platform");
    plan.matrix.include.push(second);

    let owner_id = plan.matrix.include[0].job_id.clone();
    assert!(owner_for_plan(&plan, &owner_id).is_err());
}

#[test]
fn owner_driver_digest_is_sorted_and_commits_each_member_driver() {
    let plan = plan_with_two_owner_members();
    let owner_id = plan.matrix.include[0].job_id.clone();
    let owner = owner_for_plan(&plan, &owner_id).expect("The owner must exist.");
    let requirements = owner_runtime_requirements(&plan, &owner)
        .expect("The owner runtime requirements must be valid.")
        .expect("The owner must have runtime requirements.");

    let mut reordered = plan.clone();
    reordered.matrix.include.reverse();
    let reordered_owner = owner_for_plan(&reordered, &owner_id).expect("The owner must exist.");
    assert_eq!(
        requirements,
        owner_runtime_requirements(&reordered, &reordered_owner)
            .expect("The reordered owner requirements must be valid.")
            .expect("The owner must have runtime requirements.")
    );

    let mut changed_driver = plan.clone();
    changed_driver.matrix.include[0].adapter_metadata["compile_driver"] =
        serde_json::json!("nextest");
    let changed_owner = owner_for_plan(&changed_driver, &owner_id).expect("The owner must exist.");
    assert_ne!(
        requirements.driver_id,
        owner_runtime_requirements(&changed_driver, &changed_owner)
            .expect("The changed owner requirements must be valid.")
            .expect("The owner must have runtime requirements.")
            .driver_id
    );
}

#[test]
fn owner_rejects_duplicate_members_and_incompatible_toolchain_pins() {
    let mut duplicate = plan_with_two_owner_members();
    duplicate.matrix.include[1].matrix_key = duplicate.matrix.include[0].matrix_key.clone();
    let owner_id = duplicate.matrix.include[0].job_id.clone();
    assert!(owner_for_plan(&duplicate, &owner_id).is_err());

    let mut changed_pin = plan_with_two_owner_members();
    let entry = &mut changed_pin.matrix.include[1];
    let changed_inputs = ToolchainInputs {
        tools: vec!["rust@1.99.1".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let changed_toolchain = toolchain_id(&changed_inputs).expect("toolchain identity");
    let ids = entry.cache_ids.as_ref().expect("cache ids");
    entry.cache_ids = Some(
        EntryCacheIds::new(
            ids.workspace_id(),
            ids.lane_id(),
            ids.platform_id(),
            &changed_toolchain,
            ids.cache_format_id(),
        )
        .expect("cache identity"),
    );
    entry.toolchain_inputs = Some(changed_inputs);
    entry.toolchain_id = Some(changed_toolchain);
    let owner = owner_for_plan(&changed_pin, &owner_id).expect("The owner must exist.");
    assert!(owner_runtime_requirements(&changed_pin, &owner).is_err());
}

fn plan_with_two_owner_members() -> Plan {
    let mut plan = plan_for_checkout(
        "/workspace/velnor",
        44,
        "a".repeat(40),
        "input-before",
        "closure-before",
    );
    let mut second = plan.matrix.include[0].clone();
    second.id = "stack:rust|task:demo-lint".to_owned();
    second.matrix_key = "matrix-demo-lint".to_owned();
    second.task_id = "demo-lint".to_owned();
    second.input_digest = digest_b3(b"second member input");
    second.adapter_metadata["task_mode"] = serde_json::json!("lint");
    plan.matrix.include.push(second);
    plan
}

fn plan_for_checkout(
    checkout: &str,
    run_id: u64,
    source_sha: String,
    input: &str,
    closure: &str,
) -> Plan {
    let run_key = run_key_for_ci(run_id, 1);
    let raw_id = package_id_for_checkout(checkout);
    Plan {
        schema: Plan::SCHEMA,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key).expect("plan id"),
        base: None,
        head: source_sha.clone(),
        event: WorkflowEvent::Qualification,
        qualification: Some(qualification_context(run_id, source_sha)),
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Pr,
        generator: PlanGenerator {
            version: "0.1.0".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "c".repeat(64),
        },
        packages: vec![package_for_id(&raw_id)],
        obligations: vec![PlanObligation {
            task_id: "rust:demo:test".to_owned(),
            decision: ObligationDecision::Execute,
            reason: "qualification_full".to_owned(),
            task_digest: digest_b3(b"task configuration"),
            task_binding_digest: TaskBindingDigest::new(digest_b3(b"task configuration"))
                .expect("binding digest"),
            input_digest: digest_b3(input.as_bytes()),
            closure_digest: digest_b3(closure.as_bytes()),
            graph_digest: digest_b3(b"graph"),
            mbx_digest: digest_b3(b"mbx"),
            profile: "default".to_owned(),
            job_id: "rust-demo".to_owned(),
        }],
        jobs: vec![crate::workflow::PlanJob {
            job_id: "rust-demo".to_owned(),
            disposition: crate::workflow::JobDisposition::Run,
        }],
        matrix: PlanMatrix {
            include: vec![matrix_for_id(&raw_id, input)],
        },
        task_ids: vec!["rust:demo:test".to_owned()],
        warnings: Vec::new(),
        edges: Vec::new(),
    }
}

fn package_id_for_checkout(checkout: &str) -> String {
    format!("path+file://{checkout}/crates/demo#demo@0.1.0")
}

fn package_for_id(package_id: &str) -> PlanPackage {
    PlanPackage {
        package_id: package_id.to_owned(),
        name: "demo".to_owned(),
        manifest: "crates/demo/Cargo.toml".to_owned(),
        selected: true,
        reasons: vec!["changed".to_owned()],
        tasks: vec!["rust:demo:test".to_owned()],
    }
}

fn matrix_for_id(package_id: &str, input: &str) -> MatrixEntry {
    MatrixEntry {
        id: "stack:rust|task:demo-test".to_owned(),
        matrix_key: "matrix-demo-test".to_owned(),
        stack_id: "rust".to_owned(),
        task_id: "demo-test".to_owned(),
        lane_variant: None,
        run: "mise run test".to_owned(),
        task_digest: digest_b3(b"task configuration"),
        adapter_metadata: serde_json::json!({
            "package_id": package_id,
            "unit_id": package_id,
            "compile_driver": "cargo",
        }),
        execute_task_ids: ExecuteTaskIds::default(),
        input_digest: digest_b3(input.as_bytes()),
        report_id: "matrix-report-demo".to_owned(),
        job_id: "rust-demo".to_owned(),
        artifact_id: "velnor-crate-demo-a1".to_owned(),
        planned_platform: PlannedPlatform::new("ubuntu-26.04", "x86_64-unknown-linux-gnu")
            .expect("planned platform"),
        cache_ids: Some(cache_ids()),
        toolchain_inputs: Some(test_toolchain_inputs()),
        toolchain_id: Some(toolchain_id(&test_toolchain_inputs()).expect("toolchain identity")),
        declared_outputs: Vec::new(),
        test_run: Vec::new(),
    }
}

fn cache_ids() -> EntryCacheIds {
    let toolchain = toolchain_id(&test_toolchain_inputs()).expect("toolchain identity");
    EntryCacheIds::new(
        &digest_b3(b"workspace"),
        &digest_b3(b"lane"),
        &digest_b3(b"platform"),
        &toolchain,
        &digest_b3(b"cache format"),
    )
    .expect("cache identity")
}

fn test_toolchain_inputs() -> ToolchainInputs {
    ToolchainInputs {
        tools: vec!["rust@1.98.1".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    }
}

fn qualification_context(run_id: u64, source_sha: String) -> QualificationDispatch {
    QualificationDispatch {
        mode: QualificationDispatchMode::Controlled,
        campaign: "identity-test".to_owned(),
        phase: QualificationPhase::UsefulDelta,
        repository: "tailrocks/velnor".to_owned(),
        default_branch: "main".to_owned(),
        git_ref: "refs/heads/main".to_owned(),
        ref_protected: true,
        workflow_ref: "tailrocks/velnor/.github/workflows/ci.yml@refs/heads/main".to_owned(),
        workflow_sha: source_sha.clone(),
        source_sha,
        run_id,
        run_attempt: 1,
        predecessor: Some(QualificationRunRef {
            run_id: run_id - 1,
            run_attempt: 1,
        }),
        scale_set_image_source_sha: None,
    }
}
