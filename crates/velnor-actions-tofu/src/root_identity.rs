//! One admission boundary for root-bearing OpenTofu proposal fields.

use velnor_actions_contract::{ContractError, ProposedTask, component_id_for_unit};

/// Recover the normalized root only after every proposal authority agrees.
/// # Errors
/// Rejects noncanonical paths, IDs, dependencies, or divergent root fields.
pub fn normalized_root_for_proposal(task: &ProposedTask) -> Result<&str, ContractError> {
    let root = if task.identity.project_root == "." {
        ""
    } else {
        &task.identity.project_root
    };
    let kind = crate::TofuTaskKind::parse(&task.task_kind)?;
    let key = crate::key_for_root(root);
    let display = crate::display_for_root(root);
    let task_id = crate::task_id_for_root(root, kind, &task.configuration)?;
    let depends_on = match kind {
        crate::TofuTaskKind::Validate => vec![crate::task_id_for_root(
            root,
            crate::TofuTaskKind::InitForValidate,
            &task.configuration,
        )?],
        crate::TofuTaskKind::Fmt | crate::TofuTaskKind::InitForValidate => Vec::new(),
    };
    if task.stack_id != crate::STACK_ID
        || task.task_id != task_id
        || task.identity.unit_key != key
        || task.identity.unit_id != key
        || task.identity.project_root != display
        || task.identity.unit_path != display
        || task.component_id != component_id_for_unit(&key, &display)
        || task.display_name != display
        || task.payload != crate::tofu_payload_argv(kind, root)?
        || task.depends_on != depends_on
        || task.reads != [display.clone()]
        || !task.gated_by.is_empty()
    {
        return Err(ContractError::identity(
            "tofu_root",
            "proposal_root_mismatch",
        ));
    }
    Ok(root)
}
