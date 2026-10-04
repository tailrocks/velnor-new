//! npm's official content store and public provenance; indexes stay private.
//!
//! Pinned npm11.19.0 bundled cacache owns content-v2 through put/get APIs.
//! Only pure source jobs create archives; consumers always execute npm ci.

use std::collections::BTreeMap;

const NPM_CACHE: &str = "${{ runner.temp }}/velnor/native/npm";

/// Exact public payload; config, HTTP indexes, logs and temporary files excluded.
pub(crate) fn payload_paths() -> Vec<String> {
    vec![
        format!("{NPM_CACHE}/_cacache/content-v2"),
        format!("{NPM_CACHE}/public-proof-v1.json"),
    ]
}

/// npm environment configuration overrides project/user cache placement.
pub(crate) fn task_env() -> BTreeMap<String, String> {
    BTreeMap::from([("npm_config_cache".to_owned(), NPM_CACHE.to_owned())])
}

#[cfg(test)]
mod tests {
    use super::{payload_paths, task_env};

    #[test]
    fn archives_exact_content_and_provenance_without_private_indexes() {
        assert_eq!(
            payload_paths(),
            [
                "${{ runner.temp }}/velnor/native/npm/_cacache/content-v2",
                "${{ runner.temp }}/velnor/native/npm/public-proof-v1.json",
            ]
        );
        assert_eq!(
            task_env()["npm_config_cache"],
            "${{ runner.temp }}/velnor/native/npm"
        );
    }
}
