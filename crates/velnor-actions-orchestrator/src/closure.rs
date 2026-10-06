//! Complete first-party input closures with explicit unknowns.
//! Declared via `#[path]` from `internal_plan.rs`; unknown inputs forbid reuse and coverage.
//!
//! Resolution dispatches per stack to the owning adapter; the closure
//! model itself lives in the contract crate.
use std::path::Path;
#[cfg(test)]
pub(crate) use velnor_actions_contract::ClosureBuilder;
use velnor_actions_contract::{ContractError, ProposedTask, Stack, TaskInputClosure};

/// Resolve one proposed task's closure against the checkout at `root`.
///
/// Closed per-stack dispatch: each adapter resolves its own tasks.
/// Callers pass validated proposals, so unknown stacks and kind
/// spellings fail closed here instead of resolving silently.
///
/// # Errors
///
/// Returns [`ContractError`] for unregistered stacks and unknown
/// task-kind spellings.
pub(crate) fn resolve_closure_at_root(
    root: &Path,
    task: &ProposedTask,
    profile_nextest_config: Option<&str>,
    graph_digest: &str,
    toolchain_id: &str,
    platform_id: &str,
    reads: &mut velnor_actions_tofu::FileCache,
) -> Result<TaskInputClosure, ContractError> {
    match Stack::require_known(&task.stack_id)? {
        Stack::Rust => velnor_actions_rust::resolve_closure_at_root(
            root,
            task,
            profile_nextest_config,
            graph_digest,
            toolchain_id,
            platform_id,
        ),
        Stack::Tofu => velnor_actions_tofu::resolve_closure_at_root(
            root,
            task,
            graph_digest,
            toolchain_id,
            platform_id,
            reads,
        ),
    }
}
