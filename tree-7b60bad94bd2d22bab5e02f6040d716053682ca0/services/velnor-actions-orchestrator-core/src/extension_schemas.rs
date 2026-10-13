//! Stack-extension schema registry checks (cache §1).
//!
//! Unknown extension schemas disable reuse and baseline coverage. Stacks
//! map to their adapter schema; task IDs map to their stack segment.

use velnor_actions_contract::Stack;
use velnor_actions_contract::cachekey::{
    RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA, is_known_stack_extension_schema,
};

/// Adapter extension schema for one stack ID, if the stack is known.
///
/// Unknown stack IDs map to `None` (unknown), disabling reuse and
/// baseline coverage for their tasks.
#[must_use]
pub fn extension_schema_for_stack(stack_id: &str) -> Option<&'static str> {
    match Stack::from_id(stack_id) {
        Some(Stack::Rust) => Some(RUST_EXTENSION_SCHEMA),
        Some(Stack::Tofu) => Some(TOFU_EXTENSION_SCHEMA),
        Some(Stack::Mise) => Some(velnor_actions_contract::NAMED_CHECK_EXTENSION_SCHEMA),
        None => None,
    }
}

/// Stack segment of a `stack/<sid>/...` task ID, if well-formed.
#[must_use]
pub fn task_stack_segment(task_id: &str) -> Option<&str> {
    task_id
        .strip_prefix("stack/")
        .and_then(|rest| rest.split('/').next())
        .filter(|segment| !segment.is_empty())
}

/// Kind segment of a stack or internal task ID, if well-formed.
///
/// Stack IDs carry a possibly nested manifest key, so the kind is the
/// segment before the trailing configuration (after a shard suffix is
/// stripped); internal IDs are `internal/<kind>/<config>`.
#[must_use]
pub fn task_kind_segment(task_id: &str) -> Option<&str> {
    if let Some(rest) = task_id.strip_prefix("internal/") {
        let mut parts = rest.split('/');
        let kind = parts.next()?;
        return (parts.next().is_some() && parts.next().is_none() && !kind.is_empty())
            .then_some(kind);
    }
    let rest = task_id.strip_prefix("stack/")?;
    let base = velnor_actions_contract::split_shard_suffix(task_id)
        .and_then(|(base, _, _)| base.strip_prefix("stack/"))
        .unwrap_or(rest);
    let parts: Vec<&str> = base.split('/').collect();
    if parts.len() < 4 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    Some(parts[parts.len() - 2])
}

/// Manifest-key segment of a `stack/<sid>/...` task ID, if well-formed.
///
/// Keys nest (`stack/tofu/stacks/vpc/fmt/default` keys `stacks/vpc`),
/// so the key is every segment between the stack ID and the trailing
/// kind/configuration pair (a shard suffix strips first), mirroring
/// the grammar's own parse. Internal obligations carry no key.
#[must_use]
pub fn task_key_segment(task_id: &str) -> Option<String> {
    let rest = task_id.strip_prefix("stack/")?;
    let base = velnor_actions_contract::split_shard_suffix(task_id)
        .and_then(|(base, _, _)| base.strip_prefix("stack/"))
        .unwrap_or(rest);
    let parts: Vec<&str> = base.split('/').collect();
    if parts.len() < 4 || parts.iter().any(|part| part.is_empty()) {
        return None;
    }
    Some(parts[1..parts.len() - 2].join("/"))
}

/// True when `task_id` carries no unknown extension schema.
///
/// Internal obligations have no adapter extension, so nothing is
/// unknown; stack obligations need a known stack schema.
#[must_use]
pub fn coverage_schema_known(task_id: &str) -> bool {
    if task_id.starts_with("internal/") {
        return task_kind_segment(task_id).is_some();
    }
    task_stack_segment(task_id)
        .and_then(extension_schema_for_stack)
        .is_some()
}

/// True when reuse decisions may rest on `schema` (cache §1).
#[must_use]
pub fn reuse_eligible_for_schema(schema: &str) -> bool {
    is_known_stack_extension_schema(schema)
}
#[cfg(test)]
mod tests;
