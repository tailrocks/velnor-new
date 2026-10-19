//! Cache seed private operations: data-only, runner-temp scoped.
//!
//! `resolve-seed-task-v1` writes its NUL frame to stdout for the lane's
//! `xargs -0 env`; the skeleton and trim print their summaries.

use std::io::Write;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    RESOLVE_SEED_TASK_OP, SEED_SKELETON_OP, SEED_TRIM_OP, resolve_seed_task, seed_skeleton,
    seed_trim,
};

use super::{InternalOp, fail_internal};

/// The seed operation for a private tag, if it is one.
pub(super) fn op_for(tag: &str) -> Option<InternalOp> {
    match tag {
        RESOLVE_SEED_TASK_OP => Some(InternalOp::SeedTask),
        SEED_SKELETON_OP => Some(InternalOp::SeedSkeleton),
        SEED_TRIM_OP => Some(InternalOp::SeedTrim),
        _ => None,
    }
}

/// True for the seed operations (gated on runner temp only).
pub(super) fn is_seed(op: InternalOp) -> bool {
    matches!(
        op,
        InternalOp::SeedTask | InternalOp::SeedSkeleton | InternalOp::SeedTrim
    )
}

/// Run one seed operation.
pub(super) fn run(op: InternalOp) -> ExitCode {
    let result = match op {
        InternalOp::SeedTask => resolve_seed_task(),
        InternalOp::SeedSkeleton => seed_skeleton().map(String::into_bytes),
        InternalOp::SeedTrim => seed_trim().map(String::into_bytes),
        _ => return fail_internal("not_a_seed_operation"),
    };
    match result {
        Ok(bytes) => match std::io::stdout().lock().write_all(&bytes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => fail_internal(&format!("write seed output: {error}")),
        },
        Err(error) => fail_internal(&error.to_string()),
    }
}
