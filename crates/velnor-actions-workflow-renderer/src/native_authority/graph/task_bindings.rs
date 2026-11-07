// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::{LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, invalid};

pub(super) fn rust(
    input: &NativeAuthorityInput<'_>,
) -> Result<BTreeMap<String, Vec<String>>, NativeAuthorityError> {
    let mut bindings = BTreeMap::new();
    for fact in input.logical_jobs {
        let LogicalJobSource::Task(source) = &fact.source else {
            continue;
        };
        let Some(manifest) = &source.rust_manifest else {
            continue;
        };
        let key = velnor_actions_contract::manifest_key_for_cargo_manifest(manifest)?;
        let mut ids = BTreeSet::new();
        let mut shards = None;
        let mut shard_indices = BTreeSet::new();
        for task in input.rust_tasks.iter().filter(|task| {
            !task.no_targets
                && !task.unit.is_empty()
                && task.unit == source.unit
                && task.configuration == source.configuration
                && &task.manifest == manifest
        }) {
            task.validate_shard(input)?;
            if let Some((index, count)) = task.shard {
                shards = Some(count);
                shard_indices.insert(index);
            }
            let id = velnor_actions_contract::task_id_for_stack(
                velnor_actions_rust::STACK_ID,
                &key,
                task.kind.as_str(),
                &task.configuration,
                task.shard,
            )?;
            if !ids.insert(id) {
                return Err(invalid("native_rust_task_duplicate"));
            }
        }
        if shards.is_some_and(|count| shard_indices.len() != count as usize) {
            return Err(invalid("native_rust_task_shard_inventory"));
        }
        if ids.is_empty()
            || bindings
                .insert(fact.id.clone(), ids.into_iter().collect())
                .is_some()
        {
            return Err(invalid("native_rust_task_inventory"));
        }
    }
    Ok(bindings)
}
