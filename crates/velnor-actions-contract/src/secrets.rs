const EXACT_SECRET_NAMES: [&str; 4] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
];

const SECRET_SUBSTRINGS: [&str; 8] = [
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "PRIVATE_KEY",
    "API_KEY",
    "ACCESS_KEY",
];

#[must_use]
pub fn is_secret_env_name(name: &str) -> bool {
    if EXACT_SECRET_NAMES.contains(&name) {
        return true;
    }
    let upper = name.to_ascii_uppercase();
    SECRET_SUBSTRINGS
        .iter()
        .any(|needle| upper.contains(needle))
}
