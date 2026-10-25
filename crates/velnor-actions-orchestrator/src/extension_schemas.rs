use velnor_actions_contract::Stack;
use velnor_actions_contract::cachekey::{
    RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA, is_known_stack_extension_schema,
};

pub(crate) const NATIVE_HOMEBREW_CHECK_IDENTITY_SCHEMA: &str = "homebrew-native-check-identity-v2";

#[must_use]
pub fn extension_schema_for_stack(stack_id: &str) -> Option<&'static str> {
    match Stack::from_id(stack_id) {
        Some(Stack::Native) => Some(NATIVE_HOMEBREW_CHECK_IDENTITY_SCHEMA),
        Some(Stack::Rust) => Some(RUST_EXTENSION_SCHEMA),
        Some(Stack::Tofu) => Some(TOFU_EXTENSION_SCHEMA),
        Some(Stack::Mise) => Some(velnor_actions_contract::NAMED_CHECK_EXTENSION_SCHEMA),
        None => None,
    }
}

#[must_use]
pub fn task_stack_segment(task_id: &str) -> Option<&str> {
    task_id
        .strip_prefix("stack/")
        .and_then(|rest| rest.split('/').next())
        .filter(|segment| !segment.is_empty())
}

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

#[must_use]
pub fn reuse_eligible_for_schema(schema: &str) -> bool {
    is_known_stack_extension_schema(schema)
}
