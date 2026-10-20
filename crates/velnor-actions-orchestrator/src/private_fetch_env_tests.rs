use std::collections::BTreeMap;

use super::*;

fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: BTreeMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |key| map.get(key).cloned()
}

fn good_env() -> Vec<(&'static str, &'static str)> {
    vec![
        (PRIVATE_OWNER_ENV, "ChainArgos"),
        (PRIVATE_REPOSITORIES_ENV, "a-repo,b-repo"),
        (PRIVATE_SOURCES_ENV, "cargo,release-assets"),
        (PRIVATE_ROOTS_ENV, ".,tools/cli"),
        ("GITHUB_WORKSPACE", "/work"),
        ("RUNNER_TEMP", "/tmp/runner"),
    ]
}

#[test]
fn the_step_env_parses_strictly() {
    let parsed = FetchRequest::parse(&env_of(&good_env())).expect("parse");
    assert_eq!(parsed.roots, ["", "tools/cli"]);
    assert_eq!(parsed.repositories, ["a-repo", "b-repo"]);
    assert_eq!(parsed.kinds.len(), 2);
    let with = |key: &str, value: &'static str| {
        let mut env = good_env();
        env.retain(|(k, _)| *k != key);
        env.push((key, value));
        FetchRequest::parse(&env_of(&env)).is_err()
    };
    assert!(with(PRIVATE_OWNER_ENV, "bad owner"));
    assert!(with(PRIVATE_REPOSITORIES_ENV, "b-repo,a-repo"));
    assert!(with(PRIVATE_REPOSITORIES_ENV, "a-repo,a-repo"));
    assert!(with(PRIVATE_REPOSITORIES_ENV, "a/b"));
    assert!(with(PRIVATE_SOURCES_ENV, "cargo,nope"));
    assert!(with(PRIVATE_ROOTS_ENV, "../up"));
    assert!(with(PRIVATE_ROOTS_ENV, "/abs"));
    assert!(with("GITHUB_WORKSPACE", "relative"));
    assert!(with("RUNNER_TEMP", ""));
    let mut missing = good_env();
    missing.retain(|(k, _)| *k != PRIVATE_SOURCES_ENV);
    assert!(FetchRequest::parse(&env_of(&missing)).is_err());
}
