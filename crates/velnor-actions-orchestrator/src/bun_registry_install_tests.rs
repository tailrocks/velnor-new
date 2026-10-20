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
        (BUN_ROOT_ENV, "frontend"),
        (BUN_SCOPES_ENV, "@a,@chainargos"),
        (VERSION_ENV, "1.4.2"),
        ("GITHUB_WORKSPACE", "/work"),
        ("RUNNER_TEMP", "/tmp/runner"),
        (CACHE_ENV, "/tmp/runner/velnor/bun-cache"),
        ("MISE_NO_CONFIG", "1"),
        ("MISE_AUTO_INSTALL", "false"),
    ]
}

#[test]
fn the_step_env_parses_strictly() {
    let parsed = InstallRequest::parse(&env_of(&good_env())).expect("parse");
    assert_eq!(parsed.root, "frontend");
    assert_eq!(parsed.scopes, ["@a", "@chainargos"]);
    assert_eq!(parsed.mise_env.len(), 2);
    let dot = {
        let mut env = good_env();
        env.retain(|(k, _)| *k != BUN_ROOT_ENV);
        env.push((BUN_ROOT_ENV, "."));
        InstallRequest::parse(&env_of(&env)).expect("dot root")
    };
    assert_eq!(dot.root, "");
    let with = |key: &str, value: &'static str| {
        let mut env = good_env();
        env.retain(|(k, _)| *k != key);
        env.push((key, value));
        InstallRequest::parse(&env_of(&env)).is_err()
    };
    assert!(with(BUN_ROOT_ENV, "../up"));
    assert!(with(BUN_ROOT_ENV, "/abs"));
    assert!(with(VERSION_ENV, "latest"));
    assert!(with(VERSION_ENV, "1.4"));
    assert!(with("GITHUB_WORKSPACE", "relative"));
    assert!(with(CACHE_ENV, "relative"));
    assert!(with(BUN_SCOPES_ENV, ""));
    let mut missing = good_env();
    missing.retain(|(k, _)| *k != VERSION_ENV);
    assert!(InstallRequest::parse(&env_of(&missing)).is_err());
}

fn request(workspace: &Path, temp: &Path) -> InstallRequest {
    InstallRequest {
        root: "frontend".to_owned(),
        scopes: vec!["@chainargos".to_owned()],
        version: "1.4.2".to_owned(),
        workspace: workspace.to_path_buf(),
        runner_temp: temp.to_path_buf(),
        cache_dir: temp.join("velnor/bun-cache"),
        mise_env: vec![("MISE_NO_CONFIG".to_owned(), "1".to_owned())],
    }
}

#[test]
fn the_two_configs_differ_only_by_the_scope_token_and_reject_overrides() {
    let work = tempfile::tempdir().expect("workspace");
    let temp = tempfile::tempdir().expect("temp");
    fs::create_dir_all(work.path().join("frontend")).expect("root");
    fs::write(
        work.path().join("frontend/bunfig.toml"),
        "[install]\nlinker = \"isolated\"\n",
    )
    .expect("bunfig");
    let request = request(work.path(), temp.path());
    let (registry, lifecycle) = write_configs(&request).expect("configs");
    let registry = fs::read_to_string(registry).expect("registry");
    let lifecycle = fs::read_to_string(lifecycle).expect("lifecycle");
    assert!(
        registry.contains("token = \"$VELNOR_NPM_READ_TOKEN\""),
        "{registry}"
    );
    assert!(
        registry.contains("url = \"https://npm.pkg.github.com/\""),
        "{registry}"
    );
    assert_eq!(lifecycle, "[install]\nlinker = \"isolated\"\n");
    fs::write(
        work.path().join("frontend/bunfig.toml"),
        "[install]\nregistry = \"https://evil.example/\"\n",
    )
    .expect("override");
    let error = write_configs(&request).expect_err("override rejected");
    assert!(
        error.to_string().contains("bunfig_registry_override"),
        "{error}"
    );
}

#[test]
fn the_child_runs_under_the_private_fetch_policy_with_one_credential() {
    let temp = tempfile::tempdir().expect("temp");
    let request = request(Path::new("/work"), temp.path());
    let token = Token::new("ghs_Example123-token").expect("token");
    let command = install_command(
        &request,
        Path::new("/tmp/runner/velnor/bun/registry.toml"),
        &token,
    )
    .expect("command");
    let argv: Vec<String> = command
        .argv()
        .into_iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let line = argv.join(" ");
    assert!(
        line.contains("exec bun@1.4.2 -- bun install --frozen-lockfile --ignore-scripts"),
        "{line}"
    );
    assert!(line.contains("--cwd=/work/frontend"), "{line}");
    assert!(
        line.contains("--config=/tmp/runner/velnor/bun/registry.toml"),
        "{line}"
    );
    assert!(!line.contains("ghs_"), "token never in argv: {line}");
    let env = command.full_env();
    let get = |key: &str| {
        env.iter()
            .find(|(k, _)| k.to_string_lossy() == key)
            .map(|(_, v)| v.to_string_lossy().into_owned())
    };
    assert_eq!(
        get("VELNOR_NPM_READ_TOKEN").as_deref(),
        Some("ghs_Example123-token")
    );
    assert_eq!(get("VELNOR_PRIVATE_FETCH_TOKEN"), None);
    assert!(get(CACHE_ENV).is_some_and(|dir| dir.ends_with("velnor/bun-cache")));
}
