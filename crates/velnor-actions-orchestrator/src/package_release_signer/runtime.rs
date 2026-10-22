//! Small facade for signer identity and filesystem helpers.

mod filesystem;
mod identity;

pub(super) use filesystem::{
    absolute_env, read_regular_file, require_private_directory, sha256, validate_release_directory,
    write_new_file,
};
pub(super) use identity::{
    published_identity_from_env, release_companions, release_version,
    signing_identity_for_repository, signing_identity_from_env, validate_identity,
};
#[cfg(test)]
pub(super) use identity::{require_stable_context, signing_event};
