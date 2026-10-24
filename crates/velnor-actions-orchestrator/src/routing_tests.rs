use std::{error::Error, fs, time::SystemTime};

use velnor_actions_contract::config::ValidatedImageReference;
use velnor_actions_contract::{ExecutionMode, RoutingWorkflow};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::routing::{workflow_request, workflow_routing_policy};
use velnor_actions_workflow_renderer::WorkflowRoutingPolicy;

fn config() -> Result<velnor_actions_contract::VelnorConfig, Box<dyn Error>> {
    let text = r#"
schema = 2
[workflow]
name = "CI"
[execution]
default_profile = "hosted"
mode = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
"#;
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "velnor-routing-policy-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), text)?;
    let config = crate::config::load_config(&root)?;
    fs::remove_dir_all(root)?;
    Ok(config)
}

#[test]
fn hosted_qualification_uses_selector_policy_without_inventing_an_image_pin()
-> Result<(), Box<dyn Error>> {
    let mut config = config()?;
    config
        .execution
        .as_mut()
        .expect("execution config")
        .workflows
        .insert(RoutingWorkflow::Qualification);

    let policy = workflow_routing_policy(&config, None).expect("routing policy");
    let selector = config
        .execution
        .as_ref()
        .expect("execution config")
        .scale_selector()?;
    assert_eq!(
        policy,
        WorkflowRoutingPolicy::configured(ExecutionMode::Hosted, selector, None)
    );
    Ok(())
}

#[test]
fn selector_policy_carries_the_validated_scale_set_image_pin() -> Result<(), Box<dyn Error>> {
    let mut config = config()?;
    let execution = config.execution.as_mut().expect("execution config");
    execution.workflows.insert(RoutingWorkflow::Qualification);
    let image = ValidatedImageReference::parse(
        "ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )?;
    execution
        .profiles
        .get_mut(&execution.scale_set_profile)
        .expect("scale-set profile")
        .job_container_image = Some(image.clone());

    let selector = execution.scale_selector()?;
    let policy = workflow_routing_policy(&config, None)?;
    assert_eq!(
        policy,
        WorkflowRoutingPolicy::configured(ExecutionMode::Hosted, selector, Some(image))
    );
    Ok(())
}

#[test]
fn qualification_request_resolves_product_pins_for_the_paired_sdk_jobs()
-> Result<(), Box<dyn Error>> {
    let mut config = config()?;
    let execution = config.execution.as_mut().expect("execution config");
    execution.workflows.insert(RoutingWorkflow::Qualification);
    let execution = execution.clone();
    let catalog = ToolCatalog::pinned();
    let routing = workflow_routing_policy(&config, None)?;

    let request = workflow_request(&config, &execution, &catalog, "0.1.0", &routing)?;
    let pins = request
        .product_release
        .as_ref()
        .expect("hosted qualification resolves its production pins");
    assert_eq!(pins.rust_version, catalog.version(PinnedTool::Rust));
    assert_eq!(
        pins.mr_boxington_version,
        catalog.version(PinnedTool::MrBoxington)
    );
    assert_eq!(pins.qualify_mbx_action_steps.len(), 3);
    Ok(())
}
