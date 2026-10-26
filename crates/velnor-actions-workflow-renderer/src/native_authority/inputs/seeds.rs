// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{LogicalJobSource, NativeAuthorityError, NativeAuthorityInput, fetch_roots, invalid};
use std::collections::BTreeSet;

#[must_use]
pub fn unique_dependency_seed_job_ids(roots: &[String]) -> Vec<String> {
    let normal = |root: &str| {
        if root.is_empty() {
            "dependency-seed".to_owned()
        } else {
            format!("dependency-seed-{}", root.replace('/', "-"))
        }
    };
    let ids = roots.iter().map(|root| normal(root)).collect::<Vec<_>>();
    let simple = ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
        && ids
            .iter()
            .all(|id| velnor_actions_contract::validate_job_id(id).is_ok());
    roots
        .iter()
        .map(|root| {
            if simple || root.is_empty() {
                normal(root)
            } else {
                format!("dependency-seed-{}", encode_seed_root(root))
            }
        })
        .collect()
}

pub(super) fn roots(input: &NativeAuthorityInput<'_>) -> Result<Vec<String>, NativeAuthorityError> {
    let fetch = fetch_roots(input.cargo_workspaces)?;
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

fn encode_seed_root(root: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(root.len().saturating_mul(2));
    for byte in root.bytes() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
