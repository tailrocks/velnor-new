use std::collections::BTreeSet;

use velnor_actions_contract::RustStackConfig;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::discover::PlannedWorkspace;
pub(crate) use crate::inventory::WorkspaceResolve;

#[derive(Debug, Clone, Default)]
pub struct AffectedInputs {
    pub(crate) resolves: Vec<WorkspaceResolve>,
    pub(crate) rules: Vec<InputRule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InputRule {
    pub(crate) globs: Vec<String>,
    pub(crate) package_ids: BTreeSet<String>,
    pub(crate) complete: bool,
}

impl InputRule {
    pub(crate) fn matches(&self, path: &str) -> bool {
        self.globs
            .iter()
            .any(|glob| velnor_actions_contract::matches_glob(glob, path))
    }
}

pub(crate) fn resolve_input_rules(
    rust: Option<&RustStackConfig>,
    workspaces: &[PlannedWorkspace],
) -> Result<Vec<InputRule>, OrchestratorError> {
    let Some(rust) = rust else {
        return Ok(Vec::new());
    };
    let mut rules = Vec::with_capacity(rust.inputs.len());
    for rule in &rust.inputs {
        let mut package_ids = BTreeSet::new();
        for name in &rule.packages {
            let before = package_ids.len();
            for workspace in workspaces {
                for package in &workspace.record.packages {
                    if package.in_workspace && !package.external && package.name == *name {
                        package_ids.insert(package.id.clone());
                    }
                }
            }
            if package_ids.len() == before {
                return Err(OrchestratorError::config(
                    CONFIG_REL,
                    "stacks.rust.inputs.packages",
                    format!("unknown_package:{name}"),
                ));
            }
        }
        rules.push(InputRule {
            globs: rule.globs.clone(),
            package_ids,
            complete: rule.complete,
        });
    }
    Ok(rules)
}
