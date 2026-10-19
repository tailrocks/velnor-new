use std::collections::BTreeSet;

use velnor_actions_rust::SelectionBroadening;

use crate::decisions::{broadening_for_path, selection_broadens_for_path};

/// Return the warning for a change that selects the full run.
pub(super) fn for_changed_paths(changed: &BTreeSet<String>) -> Option<String> {
    for (kind, warning) in [
        (
            SelectionBroadening::Lockfile,
            "cargo_lock_changed:all_changed",
        ),
        (
            SelectionBroadening::RootConfig,
            "root_config_changed:all_changed",
        ),
    ] {
        if changed
            .iter()
            .any(|path| broadening_for_path(path) == Some(kind))
        {
            return Some(warning.to_owned());
        }
    }
    changed
        .iter()
        .find_map(|path| selection_broadens_for_path(path))
        .map(str::to_owned)
}
