//! Crate-job display-gate unit tests.
//!
//! Declared via `#[path]` from `crate_job.rs` under `cfg(test)`.

use super::*;
use crate::{digest_b3, matrix_id_for_task_group, matrix_key_for_id, task_id_for_stack};

/// Minimal valid obligation through the real identity constructors.
fn obligation() -> CrateObligation {
    let task_id =
        task_id_for_stack("tofu", "stacks/a", "validate", "default", None).expect("task id");
    let matrix_id = matrix_id_for_task_group("tofu", &task_id).expect("matrix id");
    CrateObligation {
        task_id,
        kind: "validate".to_owned(),
        step_name: "Validate".to_owned(),
        gated_by: Vec::new(),
        matrix_key: matrix_key_for_id(&matrix_id).expect("matrix key"),
        task_digest: digest_b3(b"argv"),
        run: vec![
            "tofu".to_owned(),
            "-chdir".to_owned(),
            "stacks/a".to_owned(),
            "validate".to_owned(),
        ],
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
