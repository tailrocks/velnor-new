//! Stack-extension schema registry checks (cache §1).
//!
//! Unknown extension schemas disable reuse and baseline coverage. Stacks
//! map to their adapter schema; task IDs map to their stack segment.

use velnor_actions_contract::Stack;
use velnor_actions_contract::cachekey::{RUST_EXTENSION_SCHEMA, is_known_stack_extension_schema};

/// Adapter extension schema for one stack ID, if the stack is known.
///
/// Tofu has no adapter schema until T09, so it maps to `None`
/// (unknown), disabling reuse and baseline coverage for tofu tasks.
#[must_use]
pub fn extension_schema_for_stack(stack_id: &str) -> Option<&'static str> {
    match Stack::from_id(stack_id) {
        Some(Stack::Rust) => Some(RUST_EXTENSION_SCHEMA),
        Some(Stack::Tofu) | None => None,
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
mod tests {
    use super::*;

    #[test]
    fn segments_parse_nested_keys_shards_and_internal() {
        assert_eq!(
            task_stack_segment("stack/rust/a/b/clippy/default"),
            Some("rust")
        );
        assert_eq!(
            task_kind_segment("stack/rust/a/b/clippy/default"),
            Some("clippy")
        );
        assert_eq!(
            task_kind_segment("stack/rust/root/nextest/default/shard-2-of-4"),
            Some("nextest")
        );
        assert_eq!(task_kind_segment("internal/policy/default"), Some("policy"));
        assert_eq!(task_kind_segment("stack/rust/root"), None);
        assert_eq!(task_stack_segment("internal/policy/default"), None);
        assert_eq!(task_stack_segment("bogus"), None);
    }

    #[test]
    fn known_rust_covers_but_unknown_stacks_do_not() {
        assert_eq!(
            extension_schema_for_stack("rust"),
            Some(RUST_EXTENSION_SCHEMA)
        );
        assert_eq!(extension_schema_for_stack("mise"), None);
        assert!(coverage_schema_known("stack/rust/root/clippy/default"));
        assert!(!coverage_schema_known("stack/unknown/root/test/default"));
        assert!(coverage_schema_known("internal/policy/default"));
        assert!(!coverage_schema_known("bogus"));
        assert!(reuse_eligible_for_schema(RUST_EXTENSION_SCHEMA));
        assert!(!reuse_eligible_for_schema("rust-task-v2"));
    }
}
