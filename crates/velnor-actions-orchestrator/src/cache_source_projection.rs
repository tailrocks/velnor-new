// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "cache_source_projection/restore.rs"]
mod restore;
#[path = "cache_source_projection/types.rs"]
mod types;
#[path = "cache_source_projection/validate.rs"]
mod validate;

pub use restore::{
    authenticate_entry, checkout_metadata_matches, decide, install_staged_file,
    install_staged_symlink, reconcile, restore_directory_times, restore_times, sort_deepest_first,
    verify_times,
};
pub use types::{
    CheckoutState, EntryKind, InstallOutcome, ProjectionEntry, RealMtime, RestoreDecision,
};
pub use validate::{
    is_git_path, reject_link_parents, sha256_hex, staged_path, validate_link_target,
    validate_link_target_in_root, validate_relative_path, validate_sha256,
};
