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
        ("MISE_CARGO_HOME", "/tmp/runner/velnor/cargo"),
    ]
}

#[test]
fn the_step_env_parses_strictly() {
    let parsed = FetchRequest::parse(&env_of(&good_env())).expect("parse");
    assert_eq!(parsed.roots, ["", "tools/cli"]);
    assert_eq!(parsed.repositories, ["a-repo", "b-repo"]);
    assert_eq!(parsed.kinds.len(), 2);
    assert_eq!(
        parsed.managed_cargo_home,
        std::path::PathBuf::from("/tmp/runner/velnor/cargo")
    );
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
    assert!(with("MISE_CARGO_HOME", ""));
    assert!(with("MISE_CARGO_HOME", "relative/cargo"));
    let mut ambient_only = good_env();
    ambient_only.retain(|(key, _)| *key != "MISE_CARGO_HOME");
    ambient_only.push(("CARGO_HOME", "/ambient/.cargo"));
    assert!(FetchRequest::parse(&env_of(&ambient_only)).is_err());
    let mut missing = good_env();
    missing.retain(|(k, _)| *k != PRIVATE_SOURCES_ENV);
    assert!(FetchRequest::parse(&env_of(&missing)).is_err());
}

#[test]
fn validated_managed_cargo_home_is_forwarded_to_the_private_child() {
    use std::ffi::{OsStr, OsString};
    use std::path::Path;

    let values = good_env();
    let get = env_of(&values);
    let request = FetchRequest::parse(&get).expect("The fetch environment must parse.");
    let owned_env =
        owned_fetch_env(&request, &get).expect("The child environment must be constructed.");
    let temp = tempfile::tempdir().expect("The test directory must exist.");
    let session = Session::create(
        temp.path(),
        Token::new("token").expect("The test token must be valid."),
        "ChainArgos",
        &["a-repo".to_owned()],
        owned_env,
    )
    .expect("The private fetch session must be created.");
    let command = session
        .cargo_fetch_command(Path::new("/work/Cargo.toml"), "rust@1.99.1")
        .expect("The Cargo fetch command must be created.");
    let child_env = command.spawn_env(&[
        (
            OsString::from("CARGO_HOME"),
            OsString::from("/ambient/.cargo"),
        ),
        (
            OsString::from("MISE_CARGO_HOME"),
            OsString::from("/ambient/mise-cargo"),
        ),
    ]);
    let value = |key: &str| {
        child_env
            .iter()
            .rev()
            .find(|(name, _)| name == OsStr::new(key))
            .map(|(_, value)| value.to_string_lossy().into_owned())
    };
    assert_eq!(
        value("MISE_CARGO_HOME").as_deref(),
        Some("/tmp/runner/velnor/cargo")
    );
    assert_eq!(value("CARGO_HOME"), None);
}
