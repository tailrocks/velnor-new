pub const STEP_PRIVATE_FETCH_ALLOWLIST: [&str; 2] =
    ["VELNOR_PRIVATE_FETCH_TOKEN", "VELNOR_NPM_READ_TOKEN"];

pub const STEP_FETCH_REDIRECT_DENYLIST: [&str; 4] = [
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_CONFIG",
    "BUN_CONFIG_REGISTRY",
];

#[must_use]
pub fn is_fetch_redirect_key(key: &str) -> bool {
    STEP_FETCH_REDIRECT_DENYLIST.contains(&key)
        || key.starts_with("GIT_CONFIG_")
        || key.starts_with("NPM_CONFIG_")
        || key.starts_with("npm_config_")
}
