//! Exact action-input bindings used by extracted reusable workflow jobs.

pub(super) fn is_cache_input(uses: &str, key: &str, value: &str) -> bool {
    let is_restore = uses.strip_prefix("actions/cache/restore@")
        == Some(velnor_actions_actionlint::actions::CACHE_ACTION_SHA);
    let is_save = uses.strip_prefix("actions/cache/save@")
        == Some(velnor_actions_actionlint::actions::CACHE_ACTION_SHA);
    let source_key = matches!(
        value,
        "${{ inputs.sources_key }}" | "${{inputs.sources_key}}"
    );
    let restore_keys = matches!(
        value,
        "${{ inputs.sources_restore_keys }}" | "${{inputs.sources_restore_keys}}"
    );
    ((is_restore || is_save) && key == "key" && source_key)
        || (is_restore && key == "restore-keys" && restore_keys)
}
