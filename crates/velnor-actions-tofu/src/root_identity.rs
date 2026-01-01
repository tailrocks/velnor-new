//! One admission boundary for root-bearing `OpenTofu` proposal fields.

use std::collections::BTreeMap;

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

/// Per-generation admission map for bounded filesystem root locators.
#[derive(Debug, Default)]
pub struct RootLocatorRegistry(BTreeMap<String, String>);

impl RootLocatorRegistry {
    /// Admit one exact normalized root and return its bounded locator.
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
    fn locator_collision_admission_rejects_distinct_roots() {
        let mut registry = RootLocatorRegistry::default();
        registry
            .admit_binding("forced-collision", "")
            .expect("first root admitted");
        registry
            .admit_binding("forced-collision", "")
            .expect("same root admitted");
        let error = registry
            .admit_binding("forced-collision", "root")
            .expect_err("collision rejected");
        assert!(error.to_string().contains("locator_collision"), "{error}");
        assert_eq!(registry.0["forced-collision"], "");
    }
}
