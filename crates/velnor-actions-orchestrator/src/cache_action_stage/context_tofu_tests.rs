use super::*;

fn key() -> String {
    let locator = velnor_actions_tofu::tofu_root_locator("").expect("root locator");
    format!(
        "velnor-v1-tofu-providers-payload-v3-x86_64-unknown-linux-gnu-1.13.1-{locator}-{}",
        "a".repeat(64)
    )
}

#[test]
fn normal_tofu_context_uses_its_own_stable_namespace() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let runner_temp = temp.path().join("runner-temp");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&runner_temp).expect("runner temp");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let cache_key = key();

    let first = StageContext::normal_tofu_providers(&runner_temp, &workspace, &cache_key)
        .expect("ToFu context");
    let second = StageContext::normal_tofu_providers(&runner_temp, &workspace, &cache_key)
        .expect("same ToFu context");
    let canonical_runner_temp = std::fs::canonicalize(&runner_temp).expect("canonical runner temp");

    assert_eq!(first.action_path(), second.action_path());
    let relative = first
        .action_path()
        .strip_prefix(&canonical_runner_temp)
        .expect("stage is beneath runner temp");
    let components: Vec<_> = relative.components().collect();
    assert_eq!(components.len(), 4);
    assert_eq!(components[0].as_os_str(), "velnor");
    assert_eq!(components[1].as_os_str(), "cache-staging");
    let digest = components[2].as_os_str().to_str().expect("ASCII digest");
    assert!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| { byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) })
    );
    assert_eq!(components[3].as_os_str(), "payload");
    assert!(
        first
            .action_path()
            .file_name()
            .is_some_and(|name| name == "payload")
    );
    let mise = StageContext::normal(
        &runner_temp,
        &workspace,
        &format!("mise-tools-v2-payload-v3-{}", "b".repeat(64)),
    )
    .expect("Mise context");
    assert_ne!(first.action_path(), mise.action_path());
}

#[test]
fn normal_tofu_context_rejects_old_or_malformed_keys() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let runner_temp = temp.path().join("runner-temp");
    let workspace = temp.path().join("workspace");
    std::fs::create_dir_all(&runner_temp).expect("runner temp");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let valid = key();
    let locator = velnor_actions_tofu::tofu_root_locator("").expect("root locator");
    let malformed_locator = valid.replace(&locator, "b3-short");

    for invalid in [
        "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-old",
        "velnor-v1-tofu-providers-payload-v3-unknown-1.13.1-b3-short",
        malformed_locator.as_str(),
    ] {
        assert!(StageContext::normal_tofu_providers(&runner_temp, &workspace, invalid).is_err());
    }
}
