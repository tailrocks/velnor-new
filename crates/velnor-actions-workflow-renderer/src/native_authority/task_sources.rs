// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{NativeAuthorityError, invalid};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{VelnorConfig, WorkflowTask};

#[derive(Debug, Clone)]
pub struct WorkflowSourceFact {
    pub path: String,
    pub sha256: Option<String>,
}

pub(super) fn admit(
    config: &VelnorConfig,
    facts: &[WorkflowSourceFact],
) -> Result<BTreeMap<String, Option<String>>, NativeAuthorityError> {
    let mut allowed = BTreeSet::from([
        "mise.toml".to_owned(),
        "mise.lock".to_owned(),
        "rust-toolchain.toml".to_owned(),
    ]);
    for task in &config.workflow.tasks {
        match task {
            WorkflowTask::Verification(task) => add_source(&mut allowed, &task.source),
            WorkflowTask::Build(task) => add_source(&mut allowed, &task.source),
            WorkflowTask::NativeImage(task) => {
                allowed.insert(task.script.clone());
            }
        }
    }
    let mut admitted = BTreeMap::new();
    for fact in facts {
        if !allowed.contains(&fact.path)
            || fact
                .sha256
                .as_deref()
                .is_some_and(|hash| !velnor_actions_contract::ids::is_lower_hex_len(hash, 64))
            || admitted
                .insert(fact.path.clone(), fact.sha256.clone())
                .is_some()
        {
            return Err(invalid("native_workflow_source_fact"));
        }
    }
    for path in allowed {
        if !admitted.contains_key(&path) {
            return Err(invalid("native_workflow_source_missing"));
        }
    }
    Ok(admitted)
}

fn add_source(paths: &mut BTreeSet<String>, source: &velnor_actions_contract::MiseTaskSource) {
    paths.insert(source.mise_config.clone());
    paths.insert(source.mise_lock_path());
    paths.insert(source.rust_toolchain_path());
}
