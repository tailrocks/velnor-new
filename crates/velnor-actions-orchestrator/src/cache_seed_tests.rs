//! Seeded roots, crate-job restores, and lane record selection.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::SeedLane;
use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};

use super::*;

fn task(task_id: &str, manifest: &str, program: &str) -> Step {
    let argv: Vec<String> = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.99.0",
        "--",
        program,
        "build",
        "--manifest-path",
        manifest,
    ]
    .map(str::to_owned)
    .to_vec();
    Step {
        parallel: false,
        name: task_id.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::TaskExecution {
            argv,
            env: BTreeMap::new(),
            task_id: task_id.to_owned(),
            task_digest: format!("b3-{}", "1".repeat(64)),
            matrix_id: format!("stack:rust|task:{task_id}"),
            matrix_key: format!("m-{}", "2".repeat(16)),
            report_helper_version: "0.1.0".to_owned(),
            matrix_max_parallel: None,
            toolchain_inputs: velnor_actions_contract::cachekey::ToolchainInputs {
                tools: vec!["rust@1.99.0".to_owned()],
                components: Vec::new(),
                compile_driver: program.to_owned(),
                test_runner: "cargo_test".to_owned(),
            },
        },
    }
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Rust / x".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    }
}

#[test]
fn lane_records_follow_the_owning_root_and_task_kind() {
    let jobs = vec![
        (
            "rust-app".to_owned(),
            job(vec![
                task("stack/rust/app/build/default", "app/Cargo.toml", "mbx"),
                task("stack/rust/app/clippy/default", "app/Cargo.toml", "mbx"),
                task("stack/rust/app/fmt/default", "app/Cargo.toml", "mbx"),
            ]),
        ),
        (
            "rust-lib".to_owned(),
            job(vec![task(
                "stack/rust/nested/lib/doc/default",
                "nested/lib/Cargo.toml",
                "cargo",
            )]),
        ),
    ];
    let nested = vec!["nested".to_owned()];
    let root = lane_records(&jobs, "", &nested).expect("root lanes");
    assert_eq!(
        root.keys().copied().collect::<Vec<_>>(),
        [SeedLane::Build, SeedLane::Clippy]
    );
    let build = &root[&SeedLane::Build][0];
    assert_eq!(build.task_id, "stack/rust/app/build/default");
    assert!(build.execution_digest.starts_with("b3-") && build.mbx && !build.nextest);
    let nested_lanes = lane_records(&jobs, "nested", &[]).expect("nested lanes");
    assert_eq!(
        nested_lanes.keys().copied().collect::<Vec<_>>(),
        [SeedLane::Doc]
    );
    assert!(!nested_lanes[&SeedLane::Doc][0].mbx);
}

#[test]
fn crate_jobs_restore_their_root_seed_read_only_from_the_plan() {
    let fetch = vec![String::new(), "nested".to_owned()];
    let seeded = vec!["nested".to_owned()];
    let steps = crate_restores(&fetch, &seeded, "nested/lib/Cargo.toml", true).expect("restores");
    assert_eq!(steps.len(), 2);
    let seed = &steps[1];
    assert_eq!(seed.role, Some(StepRole::DependencySeedRestore));
    let StepKind::Action { with, .. } = &seed.kind else {
        panic!("restore is an action");
    };
    assert_eq!(with["key"], "${{needs.plan.outputs.rsdeps_key_0}}");
    assert_eq!(
        with["restore-keys"],
        "${{needs.plan.outputs.rsdeps_restore_keys_0}}"
    );
    assert!(with["path"].starts_with("nested/target/debug\n"));
    let unseeded = crate_restores(&fetch, &seeded, "app/Cargo.toml", true).expect("root");
    assert_eq!(unseeded.len(), 1, "the root is not seeded");
    assert_eq!(
        crate_restores(&fetch, &seeded, "nested/lib/Cargo.toml", false)
            .expect("tofu")
            .len(),
        0
    );
}

#[test]
fn seed_job_ids_are_unique_per_root() {
    assert_eq!(job::seed_job_id(""), "cache-seed");
    assert_eq!(
        job::seed_job_id("crates/velnor-runner"),
        "cache-seed-crates-velnor-runner"
    );
    let condition = job::seed_job_condition("main");
    assert!(condition.contains("head_branch == 'main'"));
    assert!(condition.contains("head_repository.full_name == github.repository"));
    assert!(condition.contains("conclusion != 'cancelled'"));
}
