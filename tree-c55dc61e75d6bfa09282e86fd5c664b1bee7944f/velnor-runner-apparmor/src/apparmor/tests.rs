use super::{PROFILE_NAMES, ProfileRecord};

pub(super) const POLICY_SHA256: &str =
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub(super) fn enforcing_profiles() -> String {
    let mut profiles = String::new();
    for name in PROFILE_NAMES {
        profiles.push_str(name);
        profiles.push_str(" (enforce)\n");
    }
    profiles
}

pub(super) fn matching_records() -> Vec<ProfileRecord> {
    PROFILE_NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| ProfileRecord {
            directory: format!("{name}.{}", index + 12),
            name: Some((*name).to_owned()),
            raw_sha256: Some(POLICY_SHA256.to_owned()),
        })
        .collect()
}

mod policy_identity_tests;
mod profile_modes_tests;
