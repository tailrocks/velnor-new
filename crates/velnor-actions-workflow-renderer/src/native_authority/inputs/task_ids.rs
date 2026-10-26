// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    LogicalJobFact, LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, NativeTaskFact,
    admit_manifest, invalid,
};
use std::collections::{BTreeMap, BTreeSet};

type Ids = BTreeMap<(String, String), String>;

pub(super) struct TaskIds {
    rust: Ids,
    tofu: Ids,
}

impl TaskIds {
    pub(super) fn new(input: &NativeAuthorityInput<'_>) -> Self {
        let mut rust = BTreeSet::new();
        let mut tofu = BTreeSet::new();
        for fact in input.logical_jobs {
            if let LogicalJobSource::Task(task) = &fact.source {
                let values = (
                    task.unit.clone(),
                    task.display_name.clone(),
                    task.configuration.clone(),
                );
                if task.rust_manifest.is_some() {
                    rust.insert(values);
                } else {
                    tofu.insert(values);
                }
            }
        }
        Self {
            rust: velnor_actions_contract::assign_crate_job_ids(
                &rust,
                velnor_actions_contract::CRATE_JOB_ID_PREFIX,
            ),
            tofu: velnor_actions_contract::assign_crate_job_ids(
                &tofu,
                velnor_actions_contract::TOFU_JOB_ID_PREFIX,
            ),
        }
    }

    pub(super) fn validate(
        &self,
        fact: &LogicalJobFact,
        task: &NativeTaskFact,
        input: &NativeAuthorityInput<'_>,
        tofu: &mut BTreeSet<String>,
    ) -> Result<(), NativeAuthorityError> {
        let key = (task.unit.clone(), task.configuration.clone());
        let assigned = if let Some(manifest) = &task.rust_manifest {
            admit_manifest(manifest, input.cargo_workspaces, false)?;
            if task.compile_driver.is_none() || !task.tofu.is_empty() {
                return Err(invalid("native_rust_driver"));
            }
            self.rust.get(&key)
        } else {
            if task.tofu.is_empty() || task.compile_driver.is_some() {
                return Err(invalid("native_task_missing_source"));
            }
            for task in &task.tofu {
                velnor_actions_tofu::validate_normalized_root(&task.root)?;
                let id = velnor_actions_tofu::task_id_for_root(
                    &task.root,
                    task.kind,
                    &task.configuration,
                )?;
                let root_key = id
                    .split('/')
                    .nth(2)
                    .ok_or_else(|| invalid("tofu_unparsable_task_id"))?;
                if velnor_actions_tofu::root_for_key(root_key)? != task.root {
                    return Err(invalid("native_tofu_root_identity"));
                }
                tofu.insert(task.root.clone());
            }
            self.tofu.get(&key)
        };
        if assigned != Some(&fact.id) {
            return Err(invalid("native_task_job_identity"));
        }
        Ok(())
    }
}
