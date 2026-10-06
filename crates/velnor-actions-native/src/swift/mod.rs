//! Typed native Swift and Apple support sources owned by the native domain.

mod support;

pub use support::{
    DESKTOP_NATIVE_ENTRY, DESKTOP_RELEASE_PROFILE, DESKTOP_VERIFICATION_PROFILE,
    desktop_helper_paths, desktop_owned_paths, profile_file, projected_profile_file,
    support_sources, valid_profile_path,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
