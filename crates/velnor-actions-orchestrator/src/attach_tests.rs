//! Lock-acquire and pre-seed attach tests.
//!
//! Declared via `#[path]` from `attach.rs` under `cfg(test)`.

use super::*;
use crate::workflow::{CHECKOUT_USES, REQUEST_DIR};
use crate::workflow_jobs::{final_job, plan_job};
use std::collections::BTreeMap;
use velnor_actions_contract::{Concurrency, Job, Permissions, Trigger};
use velnor_actions_workflow_renderer::render::{RenderContext, WORKFLOW_PATH};

/// Minimal crate job covering the crate attach branch.
fn legacy_task_job() -> Job {
    Job {
        display_name: "Rust / demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![crate::workflow::wire_w1::checkout_step().expect("checkout")],
    }
}

/// Bare IR shell shared by the attach fixtures.
fn bare_ir(jobs: BTreeMap<String, velnor_actions_contract::Job>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "g".to_owned(),
            cancel_in_progress: "c".to_owned(),
        },
        jobs,
    }
}

#[test]
fn lock_acquire_inserts_digest_verified_stage() {
    use velnor_actions_contract::{GeneratorBinary, LockedGenerator, MiseBootstrap};
    let lock = GeneratorLock {
        schema: 1,
        generator: LockedGenerator {
            binary: "velnor-actions".to_owned(),
            version: "0.1.0".to_owned(),
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
            plan_job("ubuntu-26.04", None, &catalog, false, false, &[]).expect("plan job"),
        ),
        (
            "required".to_owned(),
            final_job("ubuntu-26.04", &[], None, &catalog).expect("final job"),
        ),
    ]));
    assert!(attach_lock_acquire(&mut ir, &lock, "ubuntu-26.04", "0.1.0").is_ok());
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
fn preseed_attach_builds_once_and_sets_mode() {
    use velnor_actions_actionlint::ActionlintConfigInput;
    use velnor_actions_workflow_renderer::{
        PRESEED_BUILD_NAME, PRESEED_DOWNLOAD_NAME, PRESEED_STAGE_NAME, PRESEED_VERIFY_MANIFEST_NAME,
    };
    let catalog = ToolCatalog::pinned();
    let mut plan = WorkflowPlan {
        ir: bare_ir(BTreeMap::from([
            (
                "plan".to_owned(),
                plan_job("ubuntu-26.04", None, &catalog, false, false, &[]).expect("plan job"),
            ),
            ("rust-demo".to_owned(), legacy_task_job()),
            (
                "required".to_owned(),
                final_job("ubuntu-26.04", &["rust-demo".to_owned()], None, &catalog)
                    .expect("final job"),
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
            candidate: None,
            preseed: false,
            plan_consumer_env: std::collections::BTreeMap::new(),
        },
        actionlint: ActionlintConfigInput::new("0.1.0").with_workflow_path(WORKFLOW_PATH),
    };
    assert!(attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").is_ok());
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
            PRESEED_BUILD_NAME,
            "Verify MBX compile (pre-seed trust-on-review)",
            "Write helper manifest (pre-seed trust-on-review)",
            "Upload helper (pre-seed trust-on-review)",
            PRESEED_STAGE_NAME,
            "Prepare Rust components",
            "Write request",
            "Plan",
        ]
    );
    for id in ["rust-demo", "required"] {
        let names: Vec<&str> = plan.ir.jobs[id]
            .steps
            .iter()
            .map(|s| s.name.as_str())
            .collect();
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
    assert!(attach_preseed(&mut plan, "ubuntu-26.04-arm", "0.1.0").is_err());
}

/// Pre-seed fixture plan over one plan job plus the final gate.
fn preseed_fixture(use_mbx: bool, fetch_roots: &[String]) -> WorkflowPlan {
    use velnor_actions_actionlint::ActionlintConfigInput;
    let catalog = ToolCatalog::pinned();
    WorkflowPlan {
        ir: bare_ir(BTreeMap::from([
            (
                "plan".to_owned(),
                plan_job("ubuntu-26.04", None, &catalog, use_mbx, false, fetch_roots)
                    .expect("plan job"),
            ),
            (
                "required".to_owned(),
                final_job("ubuntu-26.04", &[], None, &catalog).expect("final job"),
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
            candidate: None,
            preseed: false,
            plan_consumer_env: BTreeMap::new(),
        },
        actionlint: ActionlintConfigInput::new("0.1.0").with_workflow_path(WORKFLOW_PATH),
    }
}

/// Assert one plan step runs under the owned fetch homes.
fn assert_owned_homes(steps: &[Step], name: &str) {
    let step = steps
        .iter()
        .find(|step| step.name == name)
        .unwrap_or_else(|| panic!("missing {name}"));
    let velnor_actions_contract::StepKind::Shell { env, .. } = &step.kind else {
        panic!("{name} must be a shell step");
    };
    for key in ["MISE_CARGO_HOME", "MISE_RUSTUP_HOME", "RUSTUP_TOOLCHAIN"] {
        assert!(env.contains_key(key), "{name} misses {key}: {env:?}");
    }
}

#[test]
fn preseed_restores_mbx_builds_after_sources_with_homes() {
    use velnor_actions_workflow_renderer::cache_p08::SAVE_SOURCES_NAME;
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let mut plan = preseed_fixture(true, &[String::new()]);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    let at = |name: &str| {
        names
            .iter()
            .position(|step| *step == name)
            .unwrap_or_else(|| panic!("missing {name}: {names:?}"))
    };
    let (restore, mbx, probe, build, verify, save) = (
        at(RESTORE_SOURCES_NAME),
        at(MBX_RESTORE_NAME),
        at(crate::source_prep::FETCH_SOURCES_STEP),
        at(PRESEED_BUILD_NAME),
        at(PRESEED_VERIFY_NAME),
        at(SAVE_SOURCES_NAME),
    );
    assert!(
        restore < mbx && mbx < probe && probe < build && build < verify && verify < save,
        "preseed order: {names:?}"
    );
    assert_owned_homes(steps, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, PRESEED_VERIFY_NAME);
}

#[test]
fn preseed_skips_mbx_restore_for_cargo_only_plans() {
    use velnor_actions_workflow_renderer::{PRESEED_BUILD_NAME, PRESEED_VERIFY_NAME};
    let mut plan = preseed_fixture(false, &[String::new()]);
    attach_preseed(&mut plan, "ubuntu-26.04", "0.1.0").expect("attach");
    let steps = &plan.ir.jobs["plan"].steps;
    let names: Vec<&str> = steps.iter().map(|step| step.name.as_str()).collect();
    assert!(
        !names.contains(&MBX_RESTORE_NAME),
        "cargo-only plans stay rust-cache-only: {names:?}"
    );
    let probe = names
        .iter()
        .position(|step| *step == crate::source_prep::FETCH_SOURCES_STEP)
        .expect("sources step");
    let build = names
        .iter()
        .position(|step| *step == PRESEED_BUILD_NAME)
        .expect("build step");
    assert!(probe < build, "build anchors after sources: {names:?}");
    assert_owned_homes(steps, PRESEED_BUILD_NAME);
    assert_owned_homes(steps, PRESEED_VERIFY_NAME);
}
