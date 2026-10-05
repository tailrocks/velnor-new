use super::*;

fn key() -> String {
    seed_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.18",
        &["rust@1.98.1".to_owned()],
    )
    .expect("key")
}

fn scratch(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("velnor-tool-seed-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("scratch");
    path
}

fn run_key(script: &str, home: &std::path::Path, seed_key: Option<&str>) -> String {
    let mut command = std::process::Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .env("HOME", home)
        .env("RUNNER_TEMP", home.join("rt"));
    if let Some(seed_key) = seed_key {
        command.env("SEED_KEY", seed_key);
    }
    let output = command.output().expect("bash");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn matching_seed_copies_mise_and_rustup_and_keeps_the_seed() {
    let root = scratch("hit");
    let home = root.join("home");
    std::fs::create_dir_all(home.join("keep")).expect("home");
    let seed = root.join("seed");
    let cache_key = key();
    std::fs::create_dir_all(seed.join("mise/tree/installs")).expect("tree");
    std::fs::create_dir_all(seed.join("rustup/tree/toolchains")).expect("rustup");
    std::fs::write(seed.join("mise/KEY"), &cache_key).expect("key file");
    std::fs::write(seed.join("mise/tree/installs/marker"), "mise-bytes").expect("marker");
    std::fs::write(seed.join("rustup/tree/toolchains/marker"), "rustup-bytes").expect("marker");
    let script = tool_seed_action_script(seed.to_str().expect("utf8")).expect("script");
    let text = run_key(&script, &home, Some(&cache_key));
    assert!(text.contains("tool seed restored share-dir"), "{text}");
    assert!(text.contains("tool seed restored toolchain-dir"), "{text}");
    assert_eq!(
        std::fs::read_to_string(home.join(".local/share/mise/installs/marker")).expect("copy"),
        "mise-bytes"
    );
    assert_eq!(
        std::fs::read_to_string(home.join("rt/velnor/rustup/toolchains/marker")).expect("copy"),
        "rustup-bytes"
    );
    assert_eq!(
        std::fs::read_to_string(seed.join("mise/tree/installs/marker")).expect("seed"),
        "mise-bytes"
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn wrong_key_and_absent_seed_do_not_copy() {
    let root = scratch("miss");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let seed = root.join("seed");
    let cache_key = key();
    std::fs::create_dir_all(seed.join("mise/tree")).expect("tree");
    std::fs::write(seed.join("mise/KEY"), "mise-v1-other-key-0123456789abcdef").expect("key");
    std::fs::write(seed.join("mise/tree/marker"), "secret").expect("marker");
    let wrong = tool_seed_action_script(seed.to_str().expect("utf8")).expect("script");
    let text = run_key(&wrong, &home, Some(&cache_key));
    assert!(text.contains("tool seed key mismatch"), "{text}");
    assert!(!home.join(".local/share/mise/marker").exists());
    assert_eq!(
        std::fs::read_to_string(seed.join("mise/tree/marker")).expect("seed"),
        "secret"
    );
    let absent =
        tool_seed_action_script(root.join("missing").to_str().expect("utf8")).expect("script");
    let text = run_key(&absent, &home, Some(&cache_key));
    assert!(text.contains("tool seed absent"), "{text}");
    assert!(tool_seed_action_script("relative").is_err());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn action_script_reads_seed_key_and_the_action_file_keeps_the_seed() {
    let root = scratch("action");
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("home");
    let seed = root.join("seed");
    let cache_key = key();
    std::fs::create_dir_all(seed.join("mise/tree")).expect("tree");
    std::fs::write(seed.join("mise/KEY"), &cache_key).expect("key");
    std::fs::write(seed.join("mise/tree/marker"), "kept").expect("marker");
    let script = tool_seed_action_script(seed.to_str().expect("utf8")).expect("script");
    let text = run_key(&script, &home, Some(&cache_key));
    assert!(text.contains("tool seed restored share-dir"), "{text}");
    assert_eq!(
        std::fs::read_to_string(home.join(".local/share/mise/marker")).expect("copy"),
        "kept"
    );
    assert_eq!(
        std::fs::read_to_string(seed.join("mise/tree/marker")).expect("seed"),
        "kept"
    );
    let wrong = run_key(&script, &home, Some("mise-v1-other-key-0123456789abcdef"));
    assert!(wrong.contains("tool seed key mismatch"), "{wrong}");
    let file = action_file("0.1.0").expect("action");
    assert_eq!(file.path, ".github/actions/velnor-tool-seed/action.yml");
    assert!(file.bytes.contains("/opt/velnor/seed"), "{}", file.bytes);
    assert!(file.bytes.contains("$SEED_KEY"), "{}", file.bytes);
    assert!(file.bytes.contains("inputs.cache_key"), "{}", file.bytes);
    assert!(file.bytes.contains("unset "), "{}", file.bytes);
    assert!(!file.bytes.contains("rm "), "{}", file.bytes);
    std::fs::remove_dir_all(&root).ok();
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Required".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: velnor_actions_contract::JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
    }
}

fn setup() -> Step {
    crate::steps::action_step(
        "Setup Mise",
        "jdx/mise-action@0123456789abcdef0123456789abcdef01234567",
        BTreeMap::new(),
    )
    .expect("setup")
}

#[test]
fn local_seed_requires_a_prior_checkout() {
    let cache_key = key();
    let mut bare = job(vec![setup()]);
    let index = insert_before_setup(&mut bare, 0, &cache_key).expect("bare");
    assert_eq!(index, 0);
    assert!(bare.steps.iter().all(|step| step.name != TOOL_SEED_NAME));
    let checkout =
        crate::steps::checkout_step("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1")
            .expect("checkout");
    let mut checked = job(vec![checkout, setup()]);
    let index = insert_before_setup(&mut checked, 1, &cache_key).expect("checked");
    assert_eq!(index, 2);
    assert_eq!(checked.steps[1].name, TOOL_SEED_NAME);
    assert_eq!(
        checked.steps[1].condition.as_deref(),
        Some("github.event_name != 'workflow_dispatch'")
    );
    let again = insert_before_setup(&mut checked, 2, &cache_key).expect("again");
    assert_eq!(again, 2);
}
