//! Closed ordinary Rust report environment, with fixed executable-home foundations.

use super::{CompiledRustReportRecipe, RustCompilerOperation};
use crate::ContractError;
use std::collections::BTreeMap;

pub(super) fn validate(
    recipe: &CompiledRustReportRecipe,
    environment: &BTreeMap<String, String>,
) -> Result<(), ContractError> {
    let version = recipe
        .compiler_argv
        .get(5)
        .and_then(|selector| selector.rsplit_once('@'))
        .map(|(_, version)| version)
        .ok_or_else(|| invalid("rust_version"))?;
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("RUSTUP_AUTO_INSTALL", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
        ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
        ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
        ("MISE_DATA_DIR", "${{ runner.temp }}/velnor/mise"),
        ("RUSTUP_TOOLCHAIN", version),
    ] {
        if environment.get(key).map(String::as_str) != Some(value) {
            return Err(invalid("foundation_environment"));
        }
    }
    for (key, value) in environment {
        if value.contains('\0') || !allowed(key) {
            return Err(invalid("foreign_environment"));
        }
    }
    let expected_flags = (recipe.operation == RustCompilerOperation::Doc).then_some("-D warnings");
    if environment.get("RUSTDOCFLAGS").map(String::as_str) != expected_flags {
        return Err(invalid("rustdocflags"));
    }
    if let Some(downstream) = environment.get("VELNOR_DOWNSTREAM_TASK_IDS") {
        for task in downstream.split(',') {
            crate::validate_task_id(task)?;
        }
    }
    let marker = [
        "VELNOR_MATRIX_NEEDS_JOB",
        "VELNOR_MATRIX_OUTPUT",
        "VELNOR_MATRIX_MAX_PARALLEL",
    ];
    if marker.iter().any(|key| environment.contains_key(*key))
        && (environment.get(marker[0]).map(String::as_str) != Some(crate::PLAN_JOB_ID)
            || environment.get(marker[1]).map(String::as_str) != Some("covered_tasks")
            || environment
                .get(marker[2])
                .and_then(|value| value.parse::<u32>().ok())
                .is_none_or(|cap| cap == 0))
    {
        return Err(invalid("matrix_marker"));
    }
    Ok(())
}

fn allowed(key: &str) -> bool {
    matches!(
        key,
        "MISE_NO_CONFIG"
            | "MISE_NO_ENV"
            | "MISE_NO_HOOKS"
            | "MISE_LOCKFILE"
            | "RUSTUP_AUTO_INSTALL"
            | "MISE_AUTO_INSTALL"
            | "MISE_EXEC_AUTO_INSTALL"
            | "MISE_RUSTUP_HOME"
            | "MISE_CARGO_HOME"
            | "MISE_DATA_DIR"
            | "RUSTUP_TOOLCHAIN"
            | "VELNOR_TASK_ID"
            | "VELNOR_TASK_DIGEST"
            | "VELNOR_MATRIX_ID"
            | "VELNOR_MATRIX_KEY"
            | "VELNOR_RUST_FRAME_ARGV_JSON"
            | "VELNOR_RUST_FRAME_TOOLCHAIN"
            | "VELNOR_MATRIX_NEEDS_JOB"
            | "VELNOR_MATRIX_OUTPUT"
            | "VELNOR_MATRIX_MAX_PARALLEL"
            | "VELNOR_DOWNSTREAM_TASK_IDS"
            | "RUSTDOCFLAGS"
    )
}

fn invalid(reason: &str) -> ContractError {
    ContractError::identity("rust_report_wrapper", reason)
}
