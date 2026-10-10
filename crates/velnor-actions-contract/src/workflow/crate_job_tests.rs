//! Crate-job display-gate unit tests.
//!
//! Declared via `#[path]` from `crate_job.rs` under `cfg(test)`.

use super::*;
use crate::cachekey::{ToolchainInputs, toolchain_id};
use crate::workflow::crate_job::task_digest_for_execution;
use crate::{matrix_id_for_task_group, matrix_key_for_id, task_id_for_stack};

/// Minimal valid obligation through the real identity constructors.
fn obligation() -> CrateObligation {
    let task_id =
        task_id_for_stack("tofu", "stacks/a", "validate", "default", None).expect("task id");
    let matrix_id = matrix_id_for_task_group("tofu", &task_id).expect("matrix id");
    let run = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "--no-env".to_owned(),
        "--no-hooks".to_owned(),
        "exec".to_owned(),
        "opentofu@1.10.0".to_owned(),
        "--".to_owned(),
        "tofu".to_owned(),
        "-chdir".to_owned(),
        "stacks/a".to_owned(),
        "validate".to_owned(),
    ];
    let toolchain_inputs = ToolchainInputs {
        tools: vec!["opentofu@1.10.0".to_owned()],
        components: vec![format!("tofu-provider-inputs:{}", "a".repeat(64))],
        compile_driver: "tofu".to_owned(),
        test_runner: "none".to_owned(),
    };
    let toolchain_id = toolchain_id(&toolchain_inputs).expect("toolchain identity");
    let task_digest =
        task_digest_for_execution(&task_id, &run, &toolchain_id).expect("task identity");
    CrateObligation {
        task_id,
        kind: "validate".to_owned(),
        step_name: "Validate".to_owned(),
        gated_by: Vec::new(),
        matrix_key: matrix_key_for_id(&matrix_id).expect("matrix key"),
        task_digest,
        toolchain_inputs,
        run,
    }
}

/// Crate-job shell with one valid obligation.
fn job(job_id: &str, display_name: &str) -> CrateJob {
    CrateJob {
        job_id: job_id.to_owned(),
        display_name: display_name.to_owned(),
        package_name: String::new(),
        package_id: String::new(),
        manifest: String::new(),
        configuration: "default".to_owned(),
        obligations: vec![obligation()],
    }
}

#[test]
fn display_gate_partitions_by_id_namespace() {
    assert!(
        job("tofu-stacks-a", "OpenToFu — stacks/a")
            .validate()
            .is_ok(),
        "tofu display accepted under tofu-"
    );
    let err = job("tofu-stacks-a", "Rust / stacks/a")
        .validate()
        .expect_err("rust display rejected under tofu-");
    assert!(
        err.to_string().contains("bad_display:tofu-stacks-a"),
        "got {err}"
    );
    assert!(
        job("rust-demo", "Rust / demo").validate().is_ok(),
        "rust contract byte-identical"
    );
    let err = job("rust-demo", "OpenToFu — demo")
        .validate()
        .expect_err("tofu display rejected under rust-");
    assert!(
        err.to_string().contains("bad_display:rust-demo"),
        "got {err}"
    );
    assert!(
        job("tofu-stacks-a", "OpenToFu — ${{x}}")
            .validate()
            .is_err(),
        "expressions fail even with the right prefix"
    );
}

#[test]
fn crate_job_rejects_self_gate() {
    let mut job = job("tofu-stacks-a", "OpenToFu — stacks/a");
    let task_id = job.obligations[0].task_id.clone();
    job.obligations[0].gated_by.push(task_id.clone());
    let error = job.validate().expect_err("self gate rejected");
    assert!(
        error
            .to_string()
            .contains(&format!("unordered_gate:{task_id}:{task_id}")),
        "{error}"
    );
}

#[test]
fn crate_job_accepts_only_strictly_prior_gates() {
    let mut job = job("tofu-stacks-a", "OpenToFu — stacks/a");
    let mut later = obligation();
    later.task_id =
        task_id_for_stack("tofu", "stacks/a", "plan", "default", None).expect("task id");
    later.gated_by.push(job.obligations[0].task_id.clone());
    job.obligations.push(later);
    assert!(job.validate().is_ok(), "prior gate accepted");
    job.obligations.swap(0, 1);
    let error = job.validate().expect_err("forward gate rejected");
    assert!(error.to_string().contains("unordered_gate:"), "{error}");
}

#[test]
fn crate_job_rejects_duplicate_before_checking_gates() {
    let mut job = job("tofu-stacks-a", "OpenToFu — stacks/a");
    let mut duplicate = job.obligations[0].clone();
    duplicate
        .gated_by
        .push(task_id_for_stack("tofu", "stacks/a", "plan", "default", None).expect("task id"));
    let task_id = duplicate.task_id.clone();
    job.obligations.push(duplicate);
    let error = job.validate().expect_err("duplicate rejected");
    assert!(
        error
            .to_string()
            .contains(&format!("duplicate_task:{task_id}")),
        "{error}"
    );
}
