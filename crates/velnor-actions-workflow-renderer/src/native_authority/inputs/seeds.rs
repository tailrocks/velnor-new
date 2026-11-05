// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, fetch_roots, invalid};
use std::collections::BTreeSet;
use velnor_actions_contract::CompiledStateMode;
use velnor_actions_contract::workflow::cache_family::SliceId;
use velnor_actions_contract::workflow::state_seal::state_producer_job_id;

pub fn state_producer_job_ids(roots: &[String]) -> Result<Vec<String>, NativeAuthorityError> {
    roots
        .iter()
        .map(|root| {
            SliceId::from_root(root)
                .map(|slug| state_producer_job_id(&slug))
                .map_err(|_| invalid("native_state_root_slug"))
        })
        .collect()
}

pub(super) fn roots(input: &NativeAuthorityInput<'_>) -> Result<Vec<String>, NativeAuthorityError> {
    let fetch = fetch_roots(input.cargo_workspaces)?;
    if input.config.stacks.compiled_state() == CompiledStateMode::Off {
        return Ok(Vec::new());
    }
    let mut roots = BTreeSet::new();
    for fact in input.logical_jobs {
        if let LogicalJobSource::Task(task) = &fact.source
            && task.compile_driver == Some(velnor_actions_rust::CompileDriver::Mbx)
        {
            let manifest = task
                .rust_manifest
                .as_deref()
                .ok_or_else(|| invalid("native_mbx_task_source"))?;
            let root = fetch
                .iter()
                .filter(|root| {
                    !root.is_empty()
                        && (manifest == root.as_str() || manifest.starts_with(&format!("{root}/")))
                })
                .max_by_key(|root| root.len())
                .map_or("", String::as_str);
            roots.insert(root.to_owned());
        }
    }
    Ok(roots.into_iter().collect())
}
