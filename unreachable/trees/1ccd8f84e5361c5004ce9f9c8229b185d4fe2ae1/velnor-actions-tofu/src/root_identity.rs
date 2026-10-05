//! One admission boundary for all root-bearing `OpenTofu` proposal fields.

use velnor_actions_contract::{ContractError, ProposedTask, component_id_for_unit};

/// Recover the normalized root only after every proposal authority agrees.
/// # Errors
/// Rejects noncanonical paths, kinds, IDs, or divergent root-bearing fields.
pub fn normalized_root_for_proposal(task: &ProposedTask) -> Result<&str, ContractError> {
    let root = if task.identity.project_root == "." {
        ""
    } else {
        &task.identity.project_root
    };
    let kind = crate::TofuTaskKind::parse(&task.task_kind)?;
    let key = crate::key_for_root(root);
    let display = crate::display_for_root(root);
    let expected_id = crate::task_id_for_root(root, kind, &task.configuration)?;
    if task.stack_id != crate::STACK_ID
        || task.task_id != expected_id
        || task.identity.unit_key != key
        || task.identity.unit_id != key
        || task.identity.project_root != display
        || task.identity.unit_path != display
        || task.component_id != component_id_for_unit(&key, &display)
        || task.payload != crate::tofu_payload_argv(kind, root)?
    {
        return Err(ContractError::identity(
            "tofu_root",
            "proposal_root_mismatch",
        ));
    }
    Ok(root)
}

/// Generation-local exact root proof for every admitted filesystem locator.
#[derive(Debug, Default)]
pub struct RootLocatorRegistry(std::collections::BTreeMap<String, String>);

impl RootLocatorRegistry {
    /// Admit a root only when its bounded locator cannot alias another root.
    /// # Errors
    /// Rejects invalid roots or a locator already bound to different root bytes.
    pub fn admit(&mut self, root: &str) -> Result<String, ContractError> {
        let locator = crate::tofu_root_locator(root)?;
        self.admit_binding(&locator, root)?;
        Ok(locator)
    }

    fn admit_binding(&mut self, locator: &str, root: &str) -> Result<(), ContractError> {
        if self.0.get(locator).is_some_and(|previous| previous != root) {
            return Err(ContractError::identity("tofu_root", "locator_collision"));
        }
        self.0.insert(locator.to_owned(), root.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_collisions_reject_distinct_exact_roots() {
        let mut registry = RootLocatorRegistry::default();
        registry
            .admit_binding("forced-collision", "")
            .expect("first root");
        registry
            .admit_binding("forced-collision", "")
            .expect("same root");
        assert!(registry.admit_binding("forced-collision", "root").is_err());
        assert_eq!(registry.0["forced-collision"], "");
    }
}
