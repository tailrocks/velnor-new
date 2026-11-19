// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::build_profile::BuildProfile;
use crate::sha256::sha256_hex;
use velnor_model::CACHE_COMPAT_LENGTH;

pub const COMPAT_SCHEMA: u32 = 1;
pub const BUILD_DRIVER: &str = "cargo";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatInputs {
    pub rustc_verbose: String,
    pub target: Option<String>,
    pub profile: BuildProfile,
    pub repo_profile: String,
    pub image: String,
    pub rustflags: String,
    pub workspace: String,
    pub driver: String,
}

#[must_use]
pub fn compat(inputs: &CompatInputs) -> String {
    let profile_env = inputs
        .profile
        .env
        .iter()
        .map(|(name, value)| format!("{name}={value}\n"))
        .collect::<Vec<_>>()
        .concat();
    let schema = COMPAT_SCHEMA.to_string();
    let fields = [
        ("schema", schema.as_str()),
        ("rustc", &inputs.rustc_verbose),
        ("target", inputs.target.as_deref().unwrap_or_default()),
        ("profile-env", &profile_env),
        ("profile-config", inputs.profile.cargo_config),
        ("repo-profile", &inputs.repo_profile),
        ("image", &inputs.image),
        ("rustflags", &inputs.rustflags),
        ("workspace", &inputs.workspace),
        ("driver", &inputs.driver),
    ];
    let canonical = fields
        .iter()
        .map(|(name, value)| format!("{name}:{}:{value}\n", value.len()))
        .collect::<Vec<_>>()
        .concat();
    sha256_hex(canonical.as_bytes())[..CACHE_COMPAT_LENGTH].to_owned()
}
