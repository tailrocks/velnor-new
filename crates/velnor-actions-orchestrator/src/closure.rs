use std::path::Path;

use velnor_actions_contract::{ContractError, ProposedTask, Stack, TaskInputClosure};

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
        Stack::Mise => super::named_checks::resolve_closure(
            root,
            task,
            graph_digest,
            toolchain_id,
            platform_id,
        ),
        Stack::Native => Err(ContractError::identity(
            "native_check",
            "native_closure_requires_plan_derivation",
        )),
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
