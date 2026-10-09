//! Lock-acquire and pre-seed attach tests.
//!
//! Declared via `#[path]` from `attach.rs` under `cfg(test)`.

use super::*;

use std::collections::BTreeMap;
use velnor_actions_contract_workflow::{Job, JobTimeout};
use velnor_actions_orchestrator_workflow_ir::publish_job::baseline_publish_job;
use velnor_actions_orchestrator_workflow_ir::workflow::{CHECKOUT_USES, REQUEST_DIR};
use velnor_actions_orchestrator_workflow_ir::workflow_jobs::{final_job, plan_job};
use velnor_actions_workflow_jobs::context::RenderContext;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

/// Minimal crate job covering the crate attach branch.
fn legacy_task_job() -> Job {
    Job {
        outputs: Vec::new(),
        check_runner: None,
        display_name: "Rust / demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            velnor_actions_orchestrator_workflow_ir::workflow::wire_w1::checkout_step()
                .expect("checkout"),
        ],
    }
}

#[test]
fn lock_acquire_inserts_digest_verified_stage() {
    use velnor_actions_contract_release::{GeneratorBinary, LockedGenerator, MiseBootstrap};
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
            commit: "ab".repeat(20),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.invalid/r".to_owned(),
                sha256: "a".repeat(64),
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: "2026.9.18".to_owned(),
            artifact: "https://example.invalid/m".to_owned(),
            sha256: "b".repeat(64),
        },
        actions: Vec::new(),
    };
    let catalog = ToolCatalog::pinned();
    let mut ir = bare_ir(BTreeMap::from([
        (
            "plan".to_owned(),
            plan_job(
                "ubuntu-26.04",
                None,
                &catalog,
                true,
                false,
                false,
                false,
                &[],
            )
            .expect("plan job"),
        ),
        (
            "required".to_owned(),
            final_job("ubuntu-26.04", &[], None, &catalog).expect("final job"),
        ),
        (
            "publish-baseline".to_owned(),
            baseline_publish_job("ubuntu-26.04", "main", None).expect("publish job"),
        ),
    ]));
    assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04", "0.1.0").is_ok());
    assert_eq!(ir.jobs["publish-baseline"].steps[0].name, "Acquire Velnor");
    let names: Vec<&str> = ir.jobs["plan"]
        .steps
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Checkout",
            "Prepare pinned tools",
            "Acquire Velnor",
            "Prepare Rust components",
            "Write request",
            "Plan"
        ]
    );
    let names: Vec<&str> = ir.jobs["required"]
        .steps
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Acquire Velnor",
            "Prepare pinned tools",
            "Write request",
            "Merge reports"
        ]
    );
    assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04-arm", "0.1.0").is_err());
}

#[test]
fn lock_acquire_stages_verification_output_task_on_ubuntu_26() {
    use velnor_actions_contract_release::{GeneratorBinary, LockedGenerator, MiseBootstrap};
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
            commit: "ab".repeat(20),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.invalid/r".to_owned(),
                sha256: "a".repeat(64),
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: "2026.9.18".to_owned(),
            artifact: "https://example.invalid/m".to_owned(),
            sha256: "b".repeat(64),
        },
        actions: Vec::new(),
    };
    let catalog = ToolCatalog::pinned();
    let mut ir = bare_ir(BTreeMap::from([
        (
            "plan".to_owned(),
            plan_job(
                "ubuntu-26.04",
                None,
                &catalog,
                true,
                false,
                false,
                false,
                &[],
            )
            .expect("plan job"),
        ),
        (
            "required".to_owned(),
            final_job("ubuntu-26.04", &[], None, &catalog).expect("final job"),
        ),
        (
            "task-frontend".to_owned(),
            verification_output_job("ubuntu-26.04"),
        ),
    ]));

    attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04", "0.1.0").expect("attach lock");
    let acquire = &ir.jobs["task-frontend"].steps[1];
    assert_eq!(acquire.name, "Acquire Velnor");
    assert_eq!(acquire.role, Some(StepRole::AcquireVelnor));
}

#[test]
fn lock_acquire_records_source_commit() {
    use velnor_actions_contract_release::{GeneratorBinary, LockedGenerator, MiseBootstrap};
    use velnor_actions_contract_workflow::StepKind;
    use velnor_actions_workflow_steps::steps::RELEASE_COMMIT_ENV;
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
            commit: "cd".repeat(20),
            binaries: vec![GeneratorBinary {
                target: "x86_64-unknown-linux-gnu".to_owned(),
                artifact: "https://example.invalid/r".to_owned(),
                sha256: "a".repeat(64),
            }],
        },
        mise_bootstrap: MiseBootstrap {
            version: "2026.9.18".to_owned(),
            artifact: "https://example.invalid/m".to_owned(),
            sha256: "b".repeat(64),
        },
        actions: Vec::new(),
    };
    let catalog = ToolCatalog::pinned();
    let mut ir = bare_ir(BTreeMap::from([(
        "plan".to_owned(),
        plan_job(
            "ubuntu-26.04",
            None,
            &catalog,
            true,
            false,
            false,
            false,
            &[],
        )
        .expect("plan job"),
    )]));
    ir.jobs.insert(
        "required".to_owned(),
        final_job("ubuntu-26.04", &[], None, &catalog).expect("final job"),
    );
    attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04", "0.1.0").expect("attach");
    for id in ["plan", "required"] {
        let acquire = ir.jobs[id]
            .steps
            .iter()
            .find(|step| step.name == "Acquire Velnor")
            .expect("acquire step");
        let StepKind::Shell { env, .. } = &acquire.kind else {
            panic!("acquire must be a shell step");
        };
        assert_eq!(
            env.get(RELEASE_COMMIT_ENV).map(String::as_str),
            Some("cd".repeat(20).as_str()),
            "lock path must record the commit like the manifest path"
        );
    }
}

#[test]
fn preseed_attach_builds_once_and_sets_mode() {
    use velnor_actions_actionlint::ActionlintConfigInput;
    use velnor_actions_workflow_cache::{
        cache_steps::MBX_PREFLIGHT_NAME, cache_steps::MBX_RESTORE_NAME,
        cache_steps::MBX_VERSION_CHECK_NAME,
    };
    use velnor_actions_workflow_jobs::{PRESEED_BUILD_NAME, PRESEED_STAGE_NAME};
    let catalog = ToolCatalog::pinned();
    let mut plan = WorkflowPlan {
        ir: bare_ir(BTreeMap::from([
            (
                "plan".to_owned(),
                plan_job(
                    "ubuntu-26.04",
                    None,
                    &catalog,
                    true,
                    false,
                    false,
                    false,
                    &[],
                )
                .expect("plan job"),
            ),
            ("rust-demo".to_owned(), legacy_task_job()),
            (
                "required".to_owned(),
                final_job("ubuntu-26.04", &["rust-demo".to_owned()], None, &catalog)
                    .expect("final job"),
            ),
            (
                "publish-baseline".to_owned(),
                baseline_publish_job("ubuntu-26.04", "main", None).expect("publish job"),
            ),
        ])),
        support: None,
        context: RenderContext {
            generator_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            staged_binary: format!("{STAGED_BINARY_PREFIX}0.1.0"),
            request_dir: REQUEST_DIR.to_owned(),
            checkout_uses: CHECKOUT_USES.to_owned(),
            validator_commands: Vec::new(),
            rust_policy: None,
            candidate: None,
            preseed: false,
            verification_tasks: Vec::new(),
            plan_consumer_env: std::collections::BTreeMap::new(),
        },
        actionlint: ActionlintConfigInput::new("0.1.0").with_workflow_path(WORKFLOW_PATH),
    };
    assert!(attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0", &[]).is_ok());
    assert!(plan.context.preseed);
    let names: Vec<&str> = plan.ir.jobs["plan"]
        .steps
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "Checkout",
            "Prepare pinned tools",
            "Prepare Rust components",
            MBX_PREFLIGHT_NAME,
            MBX_RESTORE_NAME,
            MBX_VERSION_CHECK_NAME,
            PRESEED_BUILD_NAME,
            "Verify MBX compile (pre-seed trust-on-review)",
            "Write helper manifest (pre-seed trust-on-review)",
            "Upload helper (pre-seed trust-on-review)",
            PRESEED_STAGE_NAME,
            "Write request",
            "Plan",
        ]
    );
    assert_preseed_consumers(&plan);
    assert!(attach_preseed(&mut plan, "ubuntu-26.04-arm", "0.1.0", &[]).is_err());
}

#[test]
fn preseed_stages_verification_output_task_before_export() {
    use velnor_actions_workflow_jobs::{
        PRESEED_DOWNLOAD_NAME, PRESEED_STAGE_NAME, PRESEED_VERIFY_MANIFEST_NAME,
    };
    let mut plan = preseed_fixture(false, &[]);
    plan.ir.jobs.insert(
        "task-frontend".to_owned(),
        verification_output_job("ubuntu-26.04"),
    );

    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0", &[]).expect("attach pre-seed");
    let names = plan.ir.jobs["task-frontend"]
        .steps
        .iter()
        .map(|step| step.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        &names[..4],
        [
            "Checkout",
            PRESEED_DOWNLOAD_NAME,
            PRESEED_VERIFY_MANIFEST_NAME,
            PRESEED_STAGE_NAME,
        ]
    );
    assert_eq!(names.last(), Some(&"Capture declared verification outputs"));
}

fn verification_output_job(label: &str) -> Job {
    let mut job = legacy_task_job();
    job.runs_on = label.to_owned();
    job.steps
        .push(velnor_actions_workflow_steps::steps::verification_artifact_export_step("frontend"));
    job
}

/// Download, verify, then stage exactly once; never rebuild.
fn assert_consumer_triple(id: &str, names: &[&str]) {
    use velnor_actions_workflow_jobs::{
        PRESEED_BUILD_NAME, PRESEED_DOWNLOAD_NAME, PRESEED_STAGE_NAME, PRESEED_VERIFY_MANIFEST_NAME,
    };
    let position = |name: &str| names.iter().position(|step| *step == name);
    let (Some(download_at), Some(verify_at), Some(stage_at)) = (
        position(PRESEED_DOWNLOAD_NAME),
        position(PRESEED_VERIFY_MANIFEST_NAME),
        position(PRESEED_STAGE_NAME),
    ) else {
        panic!("{id} misses download/verify/stage: {names:?}");
    };
    assert!(
        download_at < verify_at && verify_at < stage_at,
        "{id} must download, verify, then stage: {names:?}"
    );
    assert!(
        !names.contains(&PRESEED_BUILD_NAME),
        "{id} must not rebuild: {names:?}"
    );
}

/// Every pre-seed consumer downloads and verifies the plan's exact helper.
fn assert_preseed_consumers(plan: &WorkflowPlan) {
    for id in ["rust-demo", "required", "publish-baseline"] {
        let names: Vec<&str> = plan.ir.jobs[id]
            .steps
            .iter()
            .map(|step| step.name.as_str())
            .collect();
        assert_consumer_triple(id, &names);
    }
}
