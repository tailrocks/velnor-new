// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

/// Directories mise fills with programs that read the repository's `mise.toml` when run.
const FRONT_DIRECTORIES: &[&str] = &["shims", "command-wrappers"];

/// Whether `candidate` (as found on `PATH`) or `resolved` (after symlinks) is a mise shim or
/// command wrapper rather than the tool itself.
///
/// Running one with the fetch token would let repository-controlled config run code with it,
/// and `env_clear` leaves it unable to trust that config anyway. Both are skipped so the
/// lookup falls through to the real binary, or fails closed when there is none.
pub(super) fn is_mise_front(candidate: &Path, resolved: &Path) -> bool {
    let in_front_directory = |path: &Path| {
        path.components().any(|part| {
            FRONT_DIRECTORIES
                .iter()
                .any(|front| part.as_os_str() == *front)
        })
    };
    in_front_directory(candidate)
        || in_front_directory(resolved)
        || resolved
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "mise" || name.starts_with("mise-"))
}
