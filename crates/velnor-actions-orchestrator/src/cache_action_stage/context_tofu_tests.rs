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
    assert_eq!(
        first.action_path().parent(),
        Some(canonical_runner_temp.as_path())
    );
    assert!(
        first
            .action_path()
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(
                |name| name.starts_with("velnor-cache-transport-v1-") && name.ends_with(".payload")
            )
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
