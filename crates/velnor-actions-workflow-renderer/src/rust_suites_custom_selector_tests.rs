use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::config::SCALE_SET_PROFILE_ID;
use velnor_actions_contract::{ExecutionConfig, ExecutionMode, RustSuiteTrigger, ScaleSetSelector};

use super::{TestResult, cache_helper_tests::verified_acquire_step, minimal_suite, sample_spec};

const SELECTOR_NAME: &str = "chainargos-sea-scale-set";

fn selector() -> Result<ScaleSetSelector, Box<dyn Error>> {
    Ok(ScaleSetSelector::try_new(
        SELECTOR_NAME,
        &["velnor".to_owned(), SELECTOR_NAME.to_owned()],
    )?)
}

#[test]
fn custom_scale_set_uses_linux_tool_cache_in_rust_suite() -> TestResult {
    let selector = selector()?;
    let mut suite = minimal_suite("nightly", vec![RustSuiteTrigger::Schedule]);
    suite.schedule = vec!["0 4 * * *".to_owned()];
    let spec = sample_spec(vec![suite]);
    let image = velnor_actions_contract::config::ValidatedImageReference::parse(
        "ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )?;
    let policy = crate::workflow_routing::WorkflowRoutingPolicy::configured(
        ExecutionMode::ScaleSet,
        selector.clone(),
        Some(image),
    );
    let helpers = BTreeMap::from([("ubuntu-24.04".to_owned(), verified_acquire_step())]);
    let files = super::render_suites_workflows_with_routing_and_helpers(&spec, &policy, &helpers)?;

    assert_eq!(files.len(), 1);
    assert!(
        files[0]
            .bytes
            .contains("runs-on: [velnor, chainargos-sea-scale-set]")
    );
    assert!(files[0].bytes.contains("name: Restore Mise tools"));
    assert!(files[0].bytes.contains("velnor-tools-cache-restore"));
    assert!(files[0].bytes.contains("name: Save Mise tools"));
    assert!(!files[0].bytes.contains("ubuntu-26.04-scale-set"));
    assert!(!files[0].bytes.contains("runs-on: ubuntu-24.04"));
    Ok(())
}

#[test]
fn execution_config_rejects_arm_and_invalid_custom_selector() -> TestResult {
    let mut config = ExecutionConfig::hosted_default("ubuntu-26.04")?;
    config.mode = Some(ExecutionMode::ScaleSet);
    let scale = config
        .profiles
        .get_mut(SCALE_SET_PROFILE_ID)
        .ok_or("scale-set profile is absent")?;
    scale.name = Some(SELECTOR_NAME.to_owned());
    scale.labels = vec!["velnor".to_owned(), SELECTOR_NAME.to_owned()];
    scale.platform = "linux/arm64".to_owned();
    assert!(config.validate("config.toml").is_err());

    let scale = config
        .profiles
        .get_mut(SCALE_SET_PROFILE_ID)
        .ok_or("scale-set profile is absent")?;
    scale.platform = "linux/amd64".to_owned();
    scale.name = Some("Invalid_Name".to_owned());
    scale.labels = vec!["velnor".to_owned(), "Invalid_Name".to_owned()];
    assert!(config.validate("config.toml").is_err());
    Ok(())
}

#[test]
fn custom_scale_set_target_uses_the_validated_linux_amd64_contract() -> TestResult {
    let selector = selector()?;
    assert_eq!(
        crate::runs_on::target_for_runner(&selector.token()),
        Some("x86_64-unknown-linux-gnu")
    );
    Ok(())
}

#[test]
fn arbitrary_self_hosted_labels_do_not_get_a_linux_target() {
    assert_eq!(crate::runs_on::target_for_runner("self-hosted"), None);
    assert_eq!(crate::runs_on::target_for_runner("custom-linux"), None);
    assert_eq!(crate::runs_on::target_for_runner("ubuntu-24.04-arm"), None);
}
